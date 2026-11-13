//! A bounded, caller-owned map from public tools to explicit plugin operations.

use crate::{Error, Executor, ToolOutput, tools::validate_name, validation::Arguments};

/// One public tool name routes to one slot and one backend operation name.
/// Routes do not grant model visibility: the caller still supplies its Registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route<'a> {
    pub name: &'a str,
    pub plugin: usize,
    pub operation: &'a str,
}

impl<'a> Route<'a> {
    pub const fn new(name: &'a str, plugin: usize, operation: &'a str) -> Self {
        Self {
            name,
            plugin,
            operation,
        }
    }
}

/// A borrowed executor and an explicit enable/disable switch. Native proxies,
/// Wasm adapters and application executors all implement the same host trait.
pub struct Slot<'a> {
    executor: &'a mut dyn Executor,
    enabled: bool,
}

impl<'a> Slot<'a> {
    pub fn new(executor: &'a mut dyn Executor) -> Self {
        Self {
            executor,
            enabled: true,
        }
    }
}

/// Routes synchronous calls without allocating, registering tools or owning
/// libraries. All targets are borrowed, so they outlive every routed call.
///
/// Construct and validate replacement plugins before calling [`Self::replace`].
/// Calls, replacement and disabling require exclusive access. There are no
/// background calls, retries, fallback targets or forced resource reclamation.
pub struct Router<'a, 's> {
    routes: &'a [Route<'a>],
    slots: &'s mut [Slot<'a>],
}

impl<'a, 's> Router<'a, 's> {
    /// Validate names, duplicate public names and every target before construction.
    pub fn new(routes: &'a [Route<'a>], slots: &'s mut [Slot<'a>]) -> Result<Self, Error> {
        for (index, route) in routes.iter().enumerate() {
            validate_name(route.name)?;
            validate_name(route.operation)?;
            if route.plugin >= slots.len() {
                return Err(Error::InvalidRange);
            }
            if routes[..index].iter().any(|prior| prior.name == route.name) {
                return Err(Error::DuplicateTool);
            }
        }
        Ok(Self { routes, slots })
    }

    /// Disabled slots fail closed with `Denied`; no backend receives the call.
    pub fn set_enabled(&mut self, plugin: usize, enabled: bool) -> Result<(), Error> {
        self.slots
            .get_mut(plugin)
            .ok_or(Error::InvalidRange)?
            .enabled = enabled;
        Ok(())
    }

    pub fn is_enabled(&self, plugin: usize) -> Result<bool, Error> {
        self.slots
            .get(plugin)
            .map(|slot| slot.enabled)
            .ok_or(Error::InvalidRange)
    }

    /// Replace only an existing slot, preserve its enabled state and return the
    /// previous borrow. Ownership, shutdown and state migration stay with the caller.
    pub fn replace(
        &mut self,
        plugin: usize,
        replacement: &'a mut dyn Executor,
    ) -> Result<&'a mut dyn Executor, Error> {
        let slot = self.slots.get_mut(plugin).ok_or(Error::InvalidRange)?;
        Ok(core::mem::replace(&mut slot.executor, replacement))
    }

    pub fn routes(&self) -> &[Route<'_>] {
        self.routes
    }
}

impl Executor for Router<'_, '_> {
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        let route = self
            .routes
            .iter()
            .find(|route| route.name == name)
            .ok_or(Error::UnknownTool)?;
        let slot = &mut self.slots[route.plugin];
        if !slot.enabled {
            return Err(Error::Denied);
        }
        slot.executor.execute(route.operation, arguments, output)
    }
}
