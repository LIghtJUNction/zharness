//! Independent typed event families. Every operation supports before/started/
//! completion/failure through Lifecycle; completed external facts use notify.
//! Amount units are operation-specific and must be documented by the application.
use super::{Event as LifecycleEvent, Fault, Payload};
use crate::Error;

macro_rules! domain {
    ($name:ident, $doc:literal, [$($action:ident),+ $(,)?]) => {
        #[doc = $doc]
        pub mod $name {
            use super::{Fault, LifecycleEvent, Payload};
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            #[non_exhaustive]
            pub enum Action { $($action),+ }
            impl Action { pub const ALL: &'static [Self] = &[$(Self::$action),+]; }
            #[derive(Clone, Copy, Debug)]
            pub struct Event<'a> {
                pub action: Action,
                pub payload: Payload<'a>,
                pub amount: u64,
                /// For edits/budget changes/checkpoints: caller's prior revision.
                pub revision: u64,
            }
            impl<'a> Event<'a> {
                pub const fn new(action: Action) -> Self {
                    Self { action, payload: Payload::metadata(0), amount: 0, revision: 0 }
                }
                pub const fn with_payload(mut self, payload: Payload<'a>) -> Self { self.payload = payload; self }
                pub const fn with_amount(mut self, amount: u64) -> Self { self.amount = amount; self }
                pub const fn at_revision(mut self, revision: u64) -> Self { self.revision = revision; self }
            }
            impl LifecycleEvent for Event<'_> {
                fn redacted(self) -> Self { Self { payload: self.payload.redacted(), ..self } }
                fn validate(self) -> Result<(), Fault> { Ok(()) }
            }
        }
    };
}
domain!(
    conversation,
    "Conversation acceptance/history boundaries, not UI event interception.",
    [
        Start,
        Close,
        AcceptUser,
        AppendMessage,
        EditMessage,
        DeleteMessage,
        Fork,
        Restore
    ]
);
domain!(
    prompt,
    "Application-owned prompt construction and injection.",
    [
        Build,
        SystemInstructions,
        InjectContext,
        ToolDefinitions,
        Compact,
        BudgetExceeded
    ]
);
domain!(
    request,
    "Request build/encode/send, explicit retry and transport observations.",
    [
        Build, Encode, Send, Retry, Receive, Headers, Timeout, Cancel
    ]
);
domain!(
    tool,
    "Additional validation/authorization/dispatch boundaries around existing tool primitives.",
    [
        Register,
        Unregister,
        ParseArguments,
        Validate,
        Authorize,
        Dispatch,
        Execute,
        TransformResult,
        CommitResult,
        Cancel
    ]
);
domain!(
    context,
    "Context storage and retention operations; summaries/token costs remain caller-owned.",
    [
        Load,
        Save,
        Append,
        Merge,
        Fork,
        Evict,
        Compact,
        BudgetChange,
        BudgetWarning,
        BudgetExceeded
    ]
);
domain!(
    agent,
    "Application worker/task/message boundaries, not an agent runtime.",
    [
        Start,
        Stop,
        Assign,
        Handoff,
        Join,
        Complete,
        Fail,
        Timeout,
        Recover,
        LeaseExpire,
        SendMessage,
        ReceiveMessage
    ]
);
domain!(
    persistence,
    "Wrap actual storage calls; a notification alone does not persist anything.",
    [
        SaveConversation,
        LoadConversation,
        RestoreHistory,
        Checkpoint,
        Snapshot,
        DeleteSnapshot
    ]
);
domain!(
    runtime,
    "Resource policy and observed runtime outcomes; no timers/rate limiter is implicit.",
    [
        BudgetConsume,
        BudgetExceeded,
        RateLimit,
        RateLimitExceeded,
        Retry,
        Cancel,
        Timeout,
        Error,
        Interrupt,
        Resume,
        Shutdown
    ]
);

pub mod model {
    use super::{Error, Fault, LifecycleEvent, Payload};
    /// User intent only. Supported levels and wire mappings belong to zcall/the
    /// application; no universal provider reasoning vocabulary is invented here.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Reasoning<'a> {
        Default,
        Disabled,
        Budget(u32),
        Level(&'a str),
    }
    #[derive(Clone, Copy, Debug)]
    pub struct Configuration<'a> {
        pub model: &'a str,
        pub reasoning: Reasoning<'a>,
        pub route: Option<&'a str>,
        pub options: Payload<'a>,
    }
    impl<'a> Configuration<'a> {
        pub const fn new(model: &'a str) -> Self {
            Self {
                model,
                reasoning: Reasoning::Default,
                route: None,
                options: Payload::metadata(0),
            }
        }
        pub fn validate(self) -> Result<(), Fault> {
            let valid = |s: &str| !s.is_empty() && !s.bytes().any(|b| b.is_ascii_control());
            if !valid(self.model)
                || self.route.is_some_and(|s| !valid(s))
                || matches!(self.reasoning, Reasoning::Level(s) if !valid(s))
            {
                return Err(Error::InvalidArguments { offset: 0 }.into());
            }
            Ok(())
        }
        pub fn redacted(self) -> Self {
            Self {
                options: self.options.redacted(),
                ..self
            }
        }
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[non_exhaustive]
    pub enum Action {
        Select,
        Switch,
        ReasoningChange,
        OptionsChange,
        RouteSelect,
        Fallback,
        Unavailable,
    }
    impl Action {
        pub const ALL: &'static [Self] = &[
            Self::Select,
            Self::Switch,
            Self::ReasoningChange,
            Self::OptionsChange,
            Self::RouteSelect,
            Self::Fallback,
            Self::Unavailable,
        ];
    }
    #[derive(Clone, Copy, Debug)]
    pub struct Event<'a> {
        pub action: Action,
        pub previous: Option<Configuration<'a>>,
        pub proposed: Configuration<'a>,
        pub revision: u64,
    }
    impl LifecycleEvent for Event<'_> {
        fn redacted(self) -> Self {
            Self {
                previous: self.previous.map(Configuration::redacted),
                proposed: self.proposed.redacted(),
                ..self
            }
        }
        fn validate(self) -> Result<(), Fault> {
            self.proposed.validate()
        }
    }
}

pub mod response {
    use super::{Fault, LifecycleEvent, Payload};
    use crate::zcall::v1::{BlockType, EventKey, ProviderSignature, StreamUsage, TurnOutcome};
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[non_exhaustive]
    pub enum Action {
        TurnStart,
        IdentityUpdated,
        BlockStart,
        TextDelta,
        ReasoningDelta,
        ToolArgumentsDelta,
        RefusalDelta,
        Signature,
        BlockEnd,
        ToolReady,
        Images,
        Grounding,
        Continuation,
        StopSequence,
        Usage,
        ChoiceEnd,
        Snapshot,
        BlockIncomplete,
        Loss,
        TurnEnd,
        FinalizeAssistant,
        CommitMessage,
        Cancel,
    }
    impl Action {
        pub const ALL: &'static [Self] = &[
            Self::TurnStart,
            Self::IdentityUpdated,
            Self::BlockStart,
            Self::TextDelta,
            Self::ReasoningDelta,
            Self::ToolArgumentsDelta,
            Self::RefusalDelta,
            Self::Signature,
            Self::BlockEnd,
            Self::ToolReady,
            Self::Images,
            Self::Grounding,
            Self::Continuation,
            Self::StopSequence,
            Self::Usage,
            Self::ChoiceEnd,
            Self::Snapshot,
            Self::BlockIncomplete,
            Self::Loss,
            Self::TurnEnd,
            Self::FinalizeAssistant,
            Self::CommitMessage,
            Self::Cancel,
        ];
    }
    #[derive(Clone, Copy, Debug)]
    pub struct Event<'a> {
        pub action: Action,
        pub payload: Payload<'a>,
        pub sequence: u64,
        pub key: Option<EventKey>,
        pub choice: Option<u32>,
        pub signature: Option<ProviderSignature<'a>>,
        pub block: Option<BlockType>,
        /// First nonempty text/refusal delta in this turn, not hidden reasoning.
        pub first_visible: bool,
        pub terminal: Option<TurnOutcome>,
        pub usage: Option<StreamUsage>,
    }
    impl Event<'_> {
        pub const fn new(action: Action) -> Self {
            Self {
                action,
                payload: Payload::metadata(0),
                sequence: 0,
                key: None,
                choice: None,
                signature: None,
                block: None,
                first_visible: false,
                terminal: None,
                usage: None,
            }
        }
    }
    impl LifecycleEvent for Event<'_> {
        fn redacted(self) -> Self {
            Self {
                payload: self.payload.redacted(),
                signature: None,
                ..self
            }
        }
        fn validate(self) -> Result<(), Fault> {
            Ok(())
        }
    }
}
