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
        match (load_supported, self.opts.resume.clone()) {
            (true, Some(prev)) => {
                let params = json!({
                    "sessionId": prev,
                    "cwd": self.opts.cwd.display().to_string(),
                    "mcpServers": self.opts.mcp_servers,
                });
                self.send("session/load", params, Pending::LoadSession)
                    .await
            }
            _ => self.new_session().await,
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
    pub(super) async fn answer_load(&mut self, msg: &Value) -> Result<(), String> {
        if let Some(err) = msg.get("error") {
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
        if let Some(sid) = self.opts.resume.clone() {
            self.opened(sid, msg.get("result"), true).await;
        }
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
            })
            .await;
        self.ready = true;
    }
}
