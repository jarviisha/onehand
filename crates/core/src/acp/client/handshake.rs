//! The handshake's answers: what `initialize` said the agent can do, and the
//! session it opened, fresh or loaded.

use super::*;

impl Conn {
    /// Open the session: a fresh one.
    pub(super) async fn new_session(&mut self) -> Result<(), String> {
        let params = json!({
            "cwd": self.opts.cwd.display().to_string(),
            "mcpServers": self.opts.mcp_servers,
        });
        self.send("session/new", params, Pending::NewSession).await
    }

    /// `initialize` answered: read what the agent offers and open the session.
    pub(super) async fn answer_init(&mut self, msg: &Value) -> Result<(), String> {
        // Surface a failed handshake as what it is — otherwise an
        // error response would read as "no loadSession capability"
        // and the eventual failure would blame `session/new` instead
        // of the real protocol-version/handshake problem.
        if let Some(err) = msg.get("error") {
            return Err(format!("initialize failed: {err}"));
        }
        self.auth_advice = auth_advice(msg.get("result"));
        let capabilities = msg.get("result").and_then(|r| r.get("agentCapabilities"));
        let load_supported = capabilities
            .and_then(|c| c.get("loadSession"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.images = capabilities
            .and_then(|c| c.get("promptCapabilities"))
            .and_then(|p| p.get("image"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.forkable = capabilities
            .and_then(|c| c.get("sessionCapabilities"))
            .and_then(|s| s.get("fork"))
            .is_some_and(Value::is_object);
        match self.opts.opening.clone() {
            Opening::Resume(prev) if load_supported => self.load(prev).await,
            Opening::Fresh | Opening::Resume(_) => self.new_session().await,
            Opening::Fork { .. } if !self.forkable => Err(not_forked("this agent cannot fork")),
            Opening::Fork { from, message } => {
                let params = json!({
                    "sessionId": from,
                    "cwd": self.opts.cwd.display().to_string(),
                    "mcpServers": self.opts.mcp_servers,
                    "_meta": { "jetbrains": { "air": { "fork": {
                        "version": 1,
                        "messageId": message,
                    } } } },
                });
                self.send("session/fork", params, Pending::Fork).await
            }
        }
    }

    /// Load the saved session `sid`.
    async fn load(&mut self, sid: String) -> Result<(), String> {
        let params = json!({
            "sessionId": sid,
            "cwd": self.opts.cwd.display().to_string(),
            "mcpServers": self.opts.mcp_servers,
        });
        self.send("session/load", params, Pending::LoadSession { sid })
            .await
    }

    /// `session/fork` answered with the new session's id. An agent that opened
    /// it in answering says what it offers, as `session/new` does; one that
    /// only saved it has it loaded.
    pub(super) async fn answer_fork(&mut self, msg: &Value) -> Result<(), String> {
        if let Some(err) = msg.get("error") {
            eprintln!("[acp] session/fork failed: {err}");
            return Err(not_forked(&in_words(err).0));
        }
        let result = msg.get("result");
        let Some(sid) = result
            .and_then(|r| r.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|sid| !sid.is_empty())
            .map(String::from)
        else {
            return Err(not_forked("the agent named no session"));
        };
        let open =
            result.is_some_and(|r| r.get("modes").is_some() || r.get("configOptions").is_some());
        match open {
            true => {
                self.opened(sid, result, true).await;
                Ok(())
            }
            false => {
                self.muted = true;
                self.load(sid).await
            }
        }
    }

    /// `session/new` answered.
    pub(super) async fn answer_new(&mut self, msg: &Value) -> Result<(), String> {
        if let Some(err) = msg.get("error") {
            // The whole object, code and all, for whoever reads the log.
            eprintln!("[acp] session/new failed: {err}");
            if login_required(err) {
                let advice = self.auth_advice.clone();
                let _ = self.output.send(AcpEvent::LoginRequired { advice }).await;
            }
            return Err(format!("Could not start a session: {}", in_words(err).0));
        }
        let result = msg.get("result");
        // Every prompt names the session; one the agent did not name
        // cannot be prompted at all, so it is a start that failed.
        let Some(sid) = result
            .and_then(|r| r.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|sid| !sid.is_empty())
            .map(String::from)
        else {
            return Err("Could not start a session: the agent named none".into());
        };
        self.opened(sid, result, false).await;
        Ok(())
    }

    /// `session/load` answered.
    pub(super) async fn answer_load(&mut self, msg: &Value, sid: String) -> Result<(), String> {
        self.muted = false;
        if let Some(err) = msg.get("error") {
            // A fork that cannot be loaded is not quietly swapped for a fresh
            // conversation: that would land somewhere other than where it was
            // asked to.
            if matches!(self.opts.opening, Opening::Fork { .. }) {
                eprintln!("[acp] loading the fork failed: {err}");
                return Err(not_forked(&in_words(err).0));
            }
            // A stale persisted session id (the adapter's store was
            // wiped or expired) must not brick the session — failing
            // here would refreeze the same id on every restart and
            // refail forever. Note it and fall back to a fresh
            // session; the loaded history stays as read-only context.
            eprintln!("[acp] session/load failed: {err}");
            let (message, detail) = in_words(err);
            let _ = self
                .output
                .send(AcpEvent::Error {
                    text: format!("Loading the session failed: {message}; starting a fresh one"),
                    detail,
                })
                .await;
            return self.new_session().await;
        }
        self.opened(sid, msg.get("result"), true).await;
        Ok(())
    }

    /// The session is open: name it, say what it offers, and let requests in.
    ///
    /// `session/load` returns the same session info as `session/new` (available
    /// modes + config options), so both say it here; else a *resumed* session
    /// shows no mode (permission) / model selectors.
    async fn opened(&mut self, sid: String, result: Option<&Value>, resumed: bool) {
        self.session_id = Some(sid.clone());
        let _ = self.output.send(AcpEvent::SessionId(sid)).await;
        if let Some(modes) = result.and_then(|r| r.get("modes")) {
            let _ = self.output.send(parse_modes(modes)).await;
        }
        let cfg = parse::parse_config_options(result.and_then(|r| r.get("configOptions")));
        if !cfg.is_empty() {
            let _ = self.output.send(AcpEvent::ConfigOptions(cfg)).await;
        }
        let _ = self
            .output
            .send(AcpEvent::Connected {
                tx: self.req_tx.clone(),
                resumed,
                forkable: self.forkable,
            })
            .await;
        self.ready = true;
    }
}

/// Why a fork did not open, as the session's failure says it.
fn not_forked(why: &str) -> String {
    format!("{FORK_FAILED}: {why}")
}

/// Whether an update is part of the conversation itself, which a load replays,
/// rather than news about the session.
pub(super) fn is_replay(update: &parse::Update) -> bool {
    match update {
        parse::Update::Agent { .. }
        | parse::Update::Thought(_)
        | parse::Update::User(_)
        | parse::Update::Interrupted
        | parse::Update::ToolCall(_)
        | parse::Update::ToolUpdate(_)
        | parse::Update::Plan(_)
        | parse::Update::Background(_)
        | parse::Update::SubagentSpawned { .. }
        | parse::Update::SubagentEnded { .. } => true,
        parse::Update::Commands(_)
        | parse::Update::ModeChanged(_)
        | parse::Update::ConfigOptions(_)
        | parse::Update::Usage(_) => false,
    }
}
