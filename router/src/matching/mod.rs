mod choose_view;
mod path_segment;
pub(crate) mod resolve_path;
pub use choose_view::*;
pub use path_segment::*;
mod horizontal;
mod nested;
mod vertical;
use crate::{static_routes::RegenerationFn, Method, SsrMode};
pub use horizontal::*;
pub use nested::*;
use std::{borrow::Cow, collections::HashSet, marker::PhantomData};
use tachys::{
    renderer::Renderer,
    view::{Render, RenderHtml},
};
pub use vertical::*;

#[derive(Debug)]
pub struct Routes<Children, Rndr> {
    base: Option<Cow<'static, str>>,
    children: Children,
    ty: PhantomData<Rndr>,
}

impl<Children, Rndr> Clone for Routes<Children, Rndr>
where
    Children: Clone,
{
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            children: self.children.clone(),
            ty: PhantomData,
        }
    }
}

impl<Children, Rndr> Routes<Children, Rndr> {
    pub fn new(children: Children) -> Self {
        Self {
            base: None,
            children,
            ty: PhantomData,
        }
    }

    pub fn new_with_base(
        children: Children,
        base: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            base: Some(base.into()),
            children,
            ty: PhantomData,
        }
    }
}

impl<Children, Rndr> Routes<Children, Rndr>
where
    Rndr: Renderer + 'static,
    Children: MatchNestedRoutes<Rndr>,
{
    pub fn match_route(&self, path: &str) -> Option<Children::Match> {
        let path = match &self.base {
            None => path,
            Some(base) => {
                let (base, path) = if base.starts_with('/') {
                    (base.trim_start_matches('/'), path.trim_start_matches('/'))
                } else {
                    (base.as_ref(), path)
                };
                match path.strip_prefix(base) {
                    Some(path) => path,
                    None => return None,
                }
            }
        };

        let (matched, remaining) = self.children.match_nested(path);
        let matched = matched?;

        if !(remaining.is_empty() || remaining == "/") {
            None
        } else {
            Some(matched.1)
        }
    }

    pub fn generate_routes(
        &self,
    ) -> (
        Option<&str>,
        impl IntoIterator<Item = GeneratedRouteData> + '_,
    ) {
        (self.base.as_deref(), self.children.generate_routes())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RouteMatchId(pub(crate) u16);

pub trait MatchInterface<R>
where
    R: Renderer + 'static,
{
    type Child: MatchInterface<R> + MatchParams + 'static;
    type View: Render<R> + RenderHtml<R> + Send + 'static;

    fn as_id(&self) -> RouteMatchId;

    fn as_matched(&self) -> &str;

    fn into_view_and_child(
        self,
    ) -> (impl ChooseView<R, Output = Self::View>, Option<Self::Child>);
}

pub trait MatchParams {
    type Params: IntoIterator<Item = (Cow<'static, str>, String)>;

    fn to_params(&self) -> Self::Params;
}

pub trait MatchNestedRoutes<R>
where
    R: Renderer + 'static,
{
    type Data;
    type View;
    type Match: MatchInterface<R> + MatchParams;

    fn match_nested<'a>(
        &'a self,
        path: &'a str,
    ) -> (Option<(RouteMatchId, Self::Match)>, &str);

    fn generate_routes(
        &self,
    ) -> impl IntoIterator<Item = GeneratedRouteData> + '_;
}

#[derive(Default, Debug, PartialEq)]
pub struct GeneratedRouteData {
    pub segments: Vec<PathSegment>,
    pub ssr_mode: SsrMode,
    pub methods: HashSet<Method>,
    pub regenerate: Vec<RegenerationFn>,
}

#[cfg(test)]
mod tests {
    use super::{NestedRoute, ParamSegment, Routes};
    use crate::{
        matching::MatchParams, MatchInterface, PathSegment, StaticSegment,
        WildcardSegment,
    };
    use either_of::Either;
    use tachys::renderer::dom::Dom;

    #[test]
    pub fn matches_single_root_route() {
        let routes =
            Routes::<_, Dom>::new(NestedRoute::new(StaticSegment("/"), || ()));
        let matched = routes.match_route("/");
        assert!(matched.is_some());
        // this case seems like it should match, but implementing it interferes with
        // handling trailing slash requirements accurately -- paths for the root are "/",
        // not "", in any case
        let matched = routes.match_route("");
        assert!(matched.is_none());
        let (base, paths) = routes.generate_routes();
        assert_eq!(base, None);
        let paths = paths.into_iter().map(|g| g.segments).collect::<Vec<_>>();
        assert_eq!(paths, vec![vec![PathSegment::Static("/".into())]]);
    }

    #[test]
    pub fn matches_nested_route() {
        let routes: Routes<_, Dom> =
            Routes::new(NestedRoute::new(StaticSegment(""), || "Home").child(
                NestedRoute::new(
                    (StaticSegment("author"), StaticSegment("contact")),
                    || "Contact Me",
                ),
            ));

        // route generation
        let (base, paths) = routes.generate_routes();
        assert_eq!(base, None);
        let paths = paths.into_iter().map(|g| g.segments).collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![vec![
                PathSegment::Static("".into()),
                PathSegment::Static("author".into()),
                PathSegment::Static("contact".into())
            ]]
        );

        let matched = routes.match_route("/author/contact").unwrap();
        assert_eq!(MatchInterface::<Dom>::as_matched(&matched), "");
        let (_, child) = MatchInterface::<Dom>::into_view_and_child(matched);
        assert_eq!(
            MatchInterface::<Dom>::as_matched(&child.unwrap()),
            "/author/contact"
        );
    }

    #[test]
    pub fn does_not_match_route_unless_full_param_matches() {
        let routes = Routes::<_, Dom>::new((
            NestedRoute::new(StaticSegment("/property-api"), || ()),
            NestedRoute::new(StaticSegment("/property"), || ()),
        ));
        let matched = routes.match_route("/property").unwrap();
        assert!(matches!(matched, Either::Right(_)));
    }

    #[test]
    pub fn does_not_match_incomplete_route() {
        let routes: Routes<_, Dom> =
            Routes::new(NestedRoute::new(StaticSegment(""), || "Home").child(
                NestedRoute::new(
                    (StaticSegment("author"), StaticSegment("contact")),
                    || "Contact Me",
                ),
            ));
        let matched = routes.match_route("/");
        assert!(matched.is_none());
    }

    #[test]
    pub fn chooses_between_nested_routes() {
        let routes: Routes<_, Dom> = Routes::new((
            NestedRoute::new(StaticSegment("/"), || ()).child((
                NestedRoute::new(StaticSegment(""), || ()),
                NestedRoute::new(StaticSegment("about"), || ()),
            )),
            NestedRoute::new(StaticSegment("/blog"), || ()).child((
                NestedRoute::new(StaticSegment(""), || ()),
                NestedRoute::new(
                    (StaticSegment("post"), ParamSegment("id")),
                    || (),
                ),
            )),
        ));

        // generates routes correctly
        let (base, paths) = routes.generate_routes();
        assert_eq!(base, None);
        let paths = paths.into_iter().map(|g| g.segments).collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                vec![
                    PathSegment::Static("/".into()),
                    PathSegment::Static("".into()),
                ],
                vec![
                    PathSegment::Static("/".into()),
                    PathSegment::Static("about".into())
                ],
                vec![
                    PathSegment::Static("/blog".into()),
                    PathSegment::Static("".into()),
                ],
                vec![
                    PathSegment::Static("/blog".into()),
                    PathSegment::Static("post".into()),
                    PathSegment::Param("id".into())
                ]
            ]
        );

        let matched = routes.match_route("/about").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert!(params.is_empty());
        let matched = routes.match_route("/blog").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert!(params.is_empty());
        let matched = routes.match_route("/blog/post/42").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert_eq!(params, vec![("id".into(), "42".into())]);
    }

    #[test]
    pub fn arbitrary_nested_routes() {
        let routes: Routes<_, Dom> = Routes::new_with_base(
            (
                NestedRoute::new(StaticSegment("/"), || ()).child((
                    NestedRoute::new(StaticSegment("/"), || ()),
                    NestedRoute::new(StaticSegment("about"), || ()),
                )),
                NestedRoute::new(StaticSegment("/blog"), || ()).child((
                    NestedRoute::new(StaticSegment(""), || ()),
                    NestedRoute::new(StaticSegment("category"), || ()),
                    NestedRoute::new(
                        (StaticSegment("post"), ParamSegment("id")),
                        || (),
                    ),
                )),
                NestedRoute::new(
                    (StaticSegment("/contact"), WildcardSegment("any")),
                    || (),
                ),
            ),
            "/portfolio",
        );

        // generates routes correctly
        let (base, _paths) = routes.generate_routes();
        assert_eq!(base, Some("/portfolio"));

        let matched = routes.match_route("/about");
        assert!(matched.is_none());

        let matched = routes.match_route("/portfolio/about").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert!(params.is_empty());

        let matched = routes.match_route("/portfolio/blog/post/42").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert_eq!(params, vec![("id".into(), "42".into())]);

        let matched = routes.match_route("/portfolio/contact").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert_eq!(params, vec![("any".into(), "".into())]);

        let matched = routes.match_route("/portfolio/contact/foobar").unwrap();
        let params = matched.to_params().collect::<Vec<_>>();
        assert_eq!(params, vec![("any".into(), "foobar".into())]);
    }
}

#[derive(Debug)]
pub struct PartialPathMatch<'a, ParamsIter> {
    pub(crate) remaining: &'a str,
    pub(crate) params: ParamsIter,
    pub(crate) matched: &'a str,
}

impl<'a, ParamsIter> PartialPathMatch<'a, ParamsIter> {
    pub fn new(
        remaining: &'a str,
        params: ParamsIter,
        matched: &'a str,
    ) -> Self {
        Self {
            remaining,
            params,
            matched,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.remaining.is_empty() || self.remaining == "/"
    }

    pub fn remaining(&self) -> &str {
        self.remaining
    }

    pub fn params(self) -> ParamsIter {
        self.params
    }

    pub fn matched(&self) -> &str {
        self.matched
    }
}
