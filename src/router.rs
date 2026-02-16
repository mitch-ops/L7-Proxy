#[derive(Clone)]
pub struct Route {
    pub prefix: String,
    pub upstream: String,
}

#[derive(Clone)]
pub struct Router {
    routes: Vec<Route>,
}

impl Router {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }

    pub fn match_route(&self, path: &str) -> Option<&Route> {
        // Longest prefix match
        self.routes
            .iter()
            .filter(|route| path.starts_with(&route.prefix))
            .max_by_key(|route| route.prefix.len())
    }
}