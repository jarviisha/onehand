//! A project's routed commands and the build host its agents' builds run on.

use super::{Effect, ProjectsDrive, Thing, Value};
use crate::project::ProjectId;
use crate::workspace::{HostName, RoutedCommand};

/// The most commands one project routes.
const MOST_ROUTES: usize = 20;

impl ProjectsDrive {
    /// Make `route` one of project `id`'s routed commands: in place of
    /// `replacing`, found by its words wherever it now stands, or added at
    /// the end. Refused, in words, for a project whose sessions run on a
    /// host, which builds there already, for a `replacing` removed
    /// meanwhile, for words already routed, and past the most a project
    /// routes. Written now; unchanged, nothing happens.
    pub fn put_route(
        &mut self,
        id: &ProjectId,
        route: RoutedCommand,
        replacing: Option<&RoutedCommand>,
    ) -> Result<Vec<Effect>, String> {
        let Some(project) = self.projects.get(id) else {
            return Ok(Vec::new());
        };
        if project.host.is_some() || project.offload.is_some() {
            return Err(format!(
                "{} runs its sessions on a host; it builds there already",
                project.name
            ));
        }
        let replacing = match replacing {
            Some(held) => match project.routes.iter().position(|r| r.words == held.words) {
                Some(at) => Some(at),
                None => return Err(format!("{} was removed meanwhile", held.words)),
            },
            None => None,
        };
        let taken = project
            .routes
            .iter()
            .enumerate()
            .any(|(at, held)| Some(at) != replacing && held.words == route.words);
        if taken {
            return Err(format!("{} already routes {}", project.name, route.words));
        }
        let mut routes = project.routes.clone();
        match replacing {
            Some(at) => routes[at] = route,
            None if routes.len() >= MOST_ROUTES => {
                return Err(format!(
                    "{} routes at most {MOST_ROUTES} commands",
                    project.name
                ));
            }
            None => routes.push(route),
        }
        Ok(self.set_routes(id, routes))
    }

    /// Stop project `id` routing `route`: named by what it is, not by its
    /// place. One that is gone already changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn remove_route(&mut self, id: &ProjectId, route: &RoutedCommand) -> Vec<Effect> {
        let Some(project) = self.projects.get(id) else {
            return Vec::new();
        };
        let routes = project
            .routes
            .iter()
            .filter(|held| *held != route)
            .cloned()
            .collect();
        self.set_routes(id, routes)
    }

    /// Pick the build host project `id`'s routed commands run on, or none.
    /// Refused, in words, for a host the project does not build on. Written
    /// now; unchanged, nothing happens.
    pub fn pick_build_host(
        &mut self,
        id: &ProjectId,
        host: Option<HostName>,
    ) -> Result<Vec<Effect>, String> {
        let Some(project) = self.projects.get_mut(id) else {
            return Ok(Vec::new());
        };
        if let Some(host) = &host {
            if !project.builds.iter().any(|b| &b.host == host) {
                return Err(format!("{} does not build on {host}", project.name));
            }
        }
        if project.builds_on == host {
            return Ok(Vec::new());
        }
        let before = std::mem::replace(&mut project.builds_on, host.clone());
        Ok(self.change(
            id,
            Thing::BuildsOn,
            Value::BuildsOn(before),
            Value::BuildsOn(host),
            false,
        ))
    }

    fn set_routes(&mut self, id: &ProjectId, routes: Vec<RoutedCommand>) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project.routes == routes {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.routes, routes.clone());
        self.change(
            id,
            Thing::Routes,
            Value::Routes(before),
            Value::Routes(routes),
            false,
        )
    }
}
