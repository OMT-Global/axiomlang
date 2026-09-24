    use super::*;
    use tempfile::tempdir;

    fn request(seq: i64, command: &str, arguments: Value) -> String {
        json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments
        })
        .to_string()
    }

    fn assert_observations_unavailable(session: &mut DapSession, first_seq: i64) {
        for (offset, (command, arguments)) in [
            ("threads", json!({})),
            ("stackTrace", json!({ "threadId": 1 })),
            ("scopes", json!({ "frameId": 1 })),
            (
                "variables",
                json!({ "variablesReference": LOCALS_VARIABLES_REFERENCE }),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let response = session
                .handle_message(&request(first_seq + offset as i64, command, arguments))
                .expect("observation response");

            assert_eq!(response.messages.len(), 1);
            assert_eq!(response.messages[0]["success"], json!(false));
            assert!(response.messages[0].get("body").is_none());
            assert!(
                response.messages[0]["message"].as_str().is_some_and(
                    |message| message.contains("active stopped source-simulator session")
                )
            );
        }
    }

    #[test]
    fn initialize_advertises_axiom_debug_capabilities() {
        let mut session = DapSession::default();
        let response = session
            .handle_message(&request(1, "initialize", json!({})))
            .expect("initialize");

        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.messages[0]["type"], json!("response"));
        assert_eq!(response.messages[0]["body"]["adapterID"], json!("axiom"));
        assert_eq!(
            response.messages[0]["body"]["axiomDebugging"]["processBacked"],
            json!(false)
        );
        assert_eq!(response.messages[1]["event"], json!("initialized"));
    }

    #[test]
    fn launch_breakpoints_stack_and_variables_round_trip() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "let answer: int = 42\nprint answer\n").expect("write program");
        let program_path = program.display().to_string();
        let mut session = DapSession::default();

        let launch = session
            .handle_message(&request(
                1,
                "launch",
                json!({ "program": program_path, "mode": SOURCE_SIMULATOR_MODE }),
            ))
            .expect("launch");
        assert_eq!(launch.messages[0]["success"], json!(true));
        assert_eq!(
            launch.messages[0]["body"]["axiomDebugging"]["runtimeState"],
            json!("source_simulation_stopped")
        );
        assert!(
            launch.messages[0]["body"]["axiomDebugging"]["identity"]["sourceGeneration"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert_eq!(launch.messages[1]["event"], json!("stopped"));

        let breakpoints = session
            .handle_message(&request(
                2,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 2 }]
                }),
            ))
            .expect("set breakpoints");
        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["verified"],
            json!(false)
        );
        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["axiomSourceResolved"],
            json!(true)
        );

        let continued = session
            .handle_message(&request(3, "continue", json!({ "threadId": 1 })))
            .expect("continue");
        assert_eq!(continued.messages[1]["event"], json!("stopped"));
        assert_eq!(continued.messages[1]["body"]["reason"], json!("step"));

        let threads = session
            .handle_message(&request(4, "threads", json!({})))
            .expect("threads");
        assert_eq!(threads.messages[0]["body"]["threads"][0]["id"], json!(1));

        let stack = session
            .handle_message(&request(5, "stackTrace", json!({ "threadId": 1 })))
            .expect("stack");
        assert_eq!(
            stack.messages[0]["body"]["stackFrames"][0]["line"],
            json!(2)
        );

        let scopes = session
            .handle_message(&request(6, "scopes", json!({ "frameId": 1 })))
            .expect("scopes");
        assert_eq!(
            scopes.messages[0]["body"]["scopes"][0]["variablesReference"],
            json!(LOCALS_VARIABLES_REFERENCE)
        );

        let variables = session
            .handle_message(&request(
                7,
                "variables",
                json!({ "variablesReference": LOCALS_VARIABLES_REFERENCE }),
            ))
            .expect("variables");
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["name"],
            json!("answer")
        );
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["value"],
            json!("42")
        );
        assert_eq!(
            variables.messages[0]["body"]["variables"][0]["type"],
            json!("int")
        );
    }

    #[test]
    fn observations_fail_closed_before_source_simulator_launch() {
        let mut session = DapSession::default();

        assert_observations_unavailable(&mut session, 1);
    }

    #[test]
    fn observations_fail_closed_after_source_simulator_termination() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "print 1\n").expect("write program");
        let mut session = DapSession::default();
        session
            .handle_message(&request(
                1,
                "launch",
                json!({
                    "program": program.display().to_string(),
                    "mode": SOURCE_SIMULATOR_MODE
                }),
            ))
            .expect("launch response");
        let terminated = session
            .handle_message(&request(2, "next", json!({ "threadId": 1 })))
            .expect("step response");
        assert_eq!(terminated.messages[1]["event"], json!("terminated"));

        assert_observations_unavailable(&mut session, 3);
    }

    #[test]
    fn set_breakpoints_can_resolve_source_without_verifying_runtime() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "let answer: int = 42\nprint answer\n").expect("write program");
        let mut session = DapSession::default();

        let breakpoints = session
            .handle_message(&request(
                1,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 2 }]
                }),
            ))
            .expect("set breakpoints");

        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["verified"],
            json!(false)
        );
        assert_eq!(
            breakpoints.messages[0]["body"]["breakpoints"][0]["axiomSourceResolved"],
            json!(true)
        );
    }

    #[test]
    fn launch_requires_explicit_source_simulator_opt_in() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "print 1\n").expect("write program");
        let mut session = DapSession::default();

        let response = session
            .handle_message(&request(
                1,
                "launch",
                json!({ "program": program.display().to_string() }),
            ))
            .expect("launch response");

        assert_eq!(response.messages[0]["success"], json!(false));
        assert!(
            response.messages[0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("process-backed launch is not implemented"))
        );
    }

    #[test]
    fn source_simulator_rejects_native_binary_paths() {
        let dir = tempdir().expect("tempdir");
        let binary = dir.path().join("main");
        fs::write(&binary, b"fixture binary bytes").expect("write binary fixture");
        let mut session = DapSession::default();

        let response = session
            .handle_message(&request(
                1,
                "launch",
                json!({
                    "program": binary.display().to_string(),
                    "mode": SOURCE_SIMULATOR_MODE
                }),
            ))
            .expect("launch response");

        assert_eq!(response.messages[0]["success"], json!(false));
        assert!(
            response.messages[0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("does not launch native binaries"))
        );
    }

    #[test]
    fn failed_relaunch_invalidates_the_previous_source_simulation() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "print 1\nprint 2\n").expect("write program");
        let missing = dir.path().join("missing.ax");
        let mut session = DapSession::default();

        let launched = session
            .handle_message(&request(
                1,
                "launch",
                json!({
                    "program": program.display().to_string(),
                    "mode": SOURCE_SIMULATOR_MODE
                }),
            ))
            .expect("initial launch response");
        assert_eq!(launched.messages[0]["success"], json!(true));
        session
            .handle_message(&request(
                2,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 2 }]
                }),
            ))
            .expect("set breakpoints response");
        assert!(!session.breakpoints.is_empty());

        let relaunched = session
            .handle_message(&request(
                3,
                "launch",
                json!({
                    "program": missing.display().to_string(),
                    "mode": SOURCE_SIMULATOR_MODE
                }),
            ))
            .expect("failed relaunch response");
        assert_eq!(relaunched.messages[0]["success"], json!(false));
        assert_eq!(session.runtime_state, RuntimeState::SourceSimulationTerminated);
        assert!(session.program.is_none());
        assert!(session.breakpoints.is_empty());

        let step = session
            .handle_message(&request(4, "next", json!({ "threadId": 1 })))
            .expect("step response");
        assert_eq!(step.messages[0]["success"], json!(false));
        assert!(step.messages[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("active source-simulator session")));
    }

    #[test]
    fn failed_first_launch_clears_prelaunch_breakpoints() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "print 1\n").expect("write program");
        let mut session = DapSession::default();

        session
            .handle_message(&request(
                1,
                "setBreakpoints",
                json!({
                    "source": { "path": program.display().to_string() },
                    "breakpoints": [{ "line": 1 }]
                }),
            ))
            .expect("set breakpoints response");
        assert!(!session.breakpoints.is_empty());

        let failed = session
            .handle_message(&request(
                2,
                "launch",
                json!({ "program": program.display().to_string() }),
            ))
            .expect("failed launch response");
        assert_eq!(failed.messages[0]["success"], json!(false));
        assert!(session.breakpoints.is_empty());
    }

    #[test]
    fn debug_status_never_claims_process_dwarf_or_profile_proof() {
        let mut session = DapSession::default();
        let response = session
            .handle_message(&request(1, "axiom/debugStatus", json!({})))
            .expect("debug status");
        let status = &response.messages[0]["body"];

        assert_eq!(status["schemaVersion"], NATIVE_DEBUG_STATUS_SCHEMA_VERSION);
        assert_eq!(status["processBacked"], false);
        assert_eq!(status["nativeAxiomDwarf"], false);
        assert_eq!(status["profileSymbolization"], false);
        assert_eq!(status["identity"]["binaryDigest"], Value::Null);
        assert_eq!(status["identity"]["target"], Value::Null);
        assert_eq!(status["runtimeState"], "not_started");
    }

    #[test]
    fn runtime_controls_fail_closed_without_an_active_simulator() {
        let mut session = DapSession::default();

        for (seq, command) in [(1, "continue"), (2, "next"), (3, "stepIn"), (4, "stepOut")] {
            let response = session
                .handle_message(&request(seq, command, json!({ "threadId": 1 })))
                .expect("runtime control response");

            assert_eq!(response.messages.len(), 1);
            assert_eq!(response.messages[0]["success"], json!(false));
            assert!(
                response.messages[0]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("active source-simulator session"))
            );
        }
    }

    #[test]
    fn stepping_past_source_end_terminates_with_status() {
        let dir = tempdir().expect("tempdir");
        let program = dir.path().join("main.ax");
        fs::write(&program, "print 1\n").expect("write program");
        let mut session = DapSession::default();
        session
            .handle_message(&request(
                1,
                "launch",
                json!({
                    "program": program.display().to_string(),
                    "mode": SOURCE_SIMULATOR_MODE
                }),
            ))
            .expect("launch response");

        let response = session
            .handle_message(&request(2, "next", json!({ "threadId": 1 })))
            .expect("step response");

        assert_eq!(response.messages[0]["success"], json!(true));
        assert_eq!(response.messages[1]["event"], json!("terminated"));
        assert_eq!(
            response.messages[1]["body"]["axiomDebugging"]["runtimeState"],
            json!("source_simulation_terminated")
        );
    }

    #[test]
    fn stdio_loop_reads_and_writes_framed_messages() {
        let body = request(7, "initialize", json!({}));
        let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut output = Vec::new();

        run_stdio(std::io::Cursor::new(input.into_bytes()), &mut output).expect("run stdio");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains(r#""adapterID":"axiom""#));
    }

    #[test]
    fn disconnect_stops_stdio_loop() {
        let mut session = DapSession::default();
        let response = session
            .handle_message(&request(9, "disconnect", json!({})))
            .expect("disconnect");

        assert!(response.exit);
        assert_eq!(response.messages[0]["success"], json!(true));
    }
