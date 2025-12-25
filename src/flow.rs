// Navigation helpers for multi-step interactive flow (service/project/params).

#[derive(Copy, Clone, PartialEq, Eq)]
enum FlowStep {
    Service,
    Project,
    Params,
}

pub enum RouteAction {
    ReturnService,
    ContinueProject,
}

pub struct StepTracker {
    allow_service: bool,
    allow_project: bool,
    stack: Vec<FlowStep>,
}

impl StepTracker {
    /// Build a tracker that only includes steps available in this run.
    pub fn new(service_step: bool, project_step: bool) -> Self {
        let mut stack = Vec::new();
        if service_step {
            stack.push(FlowStep::Service);
        }
        StepTracker {
            allow_service: service_step,
            allow_project: project_step,
            stack,
        }
    }

    pub fn enter_project(&mut self) {
        if !self.allow_project {
            return;
        }
        // Record that we've entered project selection (if available).
        self.push_step(FlowStep::Project);
    }

    /// Mark that we've entered the parameter step (always exists).
    pub fn enter_params(&mut self) {
        // Parameter step is always the final interactive step.
        self.push_step(FlowStep::Params);
    }

    fn back(&mut self) -> Option<RouteAction> {
        if self.stack.is_empty() {
            return None;
        }
        // Pop the current step and decide where Ctrl+C should return.
        self.stack.pop();
        match self.stack.last() {
            Some(FlowStep::Service) => Some(RouteAction::ReturnService),
            Some(FlowStep::Project) => Some(RouteAction::ContinueProject),
            Some(FlowStep::Params) => {
                // Params should never be present after popping; keep this non-fatal in release.
                debug_assert!(false, "Params should not be on the stack after popping");
                None
            }
            None => None,
        }
    }

    fn push_step(&mut self, step: FlowStep) {
        if self.stack.last() == Some(&step) {
            return;
        }
        // Only record allowed steps to keep back navigation consistent.
        if step == FlowStep::Service && !self.allow_service {
            return;
        }
        self.stack.push(step);
    }
}

pub fn handle_back_and_route(steps: &mut StepTracker, exit_msg: &str) -> RouteAction {
    // Resolve Ctrl+C back/exit for the current step.
    match steps.back() {
        Some(route) => route,
        None => {
            crate::utils::prepare_terminal_for_exit();
            println!("{}", exit_msg);
            std::process::exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StepTracker;

    #[test]
    fn back_flow_with_service_and_project() {
        let mut steps = StepTracker::new(true, true);
        steps.enter_project();
        steps.enter_params();

        // From params -> project
        let back = steps.back();
        assert!(matches!(back, Some(super::RouteAction::ContinueProject)));

        // From project -> service
        let back = steps.back();
        assert!(matches!(back, Some(super::RouteAction::ReturnService)));

        // From service -> exit
        let back = steps.back();
        assert!(back.is_none());
    }

    #[test]
    fn back_flow_without_service() {
        let mut steps = StepTracker::new(false, true);
        steps.enter_project();
        steps.enter_params();

        let back = steps.back();
        assert!(matches!(back, Some(super::RouteAction::ContinueProject)));

        let back = steps.back();
        assert!(back.is_none());
    }

    #[test]
    fn back_flow_job_url_skips_project() {
        let mut steps = StepTracker::new(true, false);
        steps.enter_project(); // should be ignored
        steps.enter_params();

        let back = steps.back();
        assert!(matches!(back, Some(super::RouteAction::ReturnService)));

        let back = steps.back();
        assert!(back.is_none());
    }
}
