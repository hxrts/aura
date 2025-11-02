      1 warning: redundant guard
      1 warning: build failed, waiting for other jobs to finish...
      1 warning: `aura-test-utils` (lib) generated 2 warnings
      1 warning: `aura-simulator` (lib) generated 442 warnings
      1 warning: `aura-protocol` (lib) generated 9 warnings (run `cargo clippy --fix --lib -p aura-protocol` to apply 7 suggestions)
      1 warning: `aura-journal` (lib) generated 1 warning
      1 warning: `aura-analysis-client` (lib) generated 1 warning
      1 warning: `aura-agent` (lib) generated 36 warnings (run `cargo clippy --fix --lib -p aura-agent` to apply 19 suggestions)
      1 crates/aura-test-utils/src/lib.rs:47:9: warning: ambiguous glob re-exports: the name `helpers` in the type namespace is first re-exported here
      1 crates/aura-test-utils/src/keys.rs:42:43: warning: use of deprecated method `sha2::digest::generic_array::GenericArray::<T, N>::as_slice`: please upgrade to generic-array 1.x
      1 crates/aura-simulator/src/utils/time.rs:7:5: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/utils/time.rs:23:5: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/utils/time.rs:15:5: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/utils/ids.rs:70:9: warning: `to_string` applied to a type that implements `Display` in `format!` args: help: use this: `&generate_random_uuid()[..8]`
      1 crates/aura-simulator/src/utils/errors.rs:192:9: warning: `crate` references the macro call's crate: help: to reference the macro definition's crate, use: `$crate`
      1 crates/aura-simulator/src/utils/checkpoints.rs:45:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/types.rs:179:1: warning: missing documentation for a type alias
      1 crates/aura-simulator/src/testing/test_utils.rs:84:13: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/testing/test_utils.rs:120:1: warning: missing documentation for a function
      1 crates/aura-simulator/src/testing/test_utils.rs:116:1: warning: missing documentation for a function
      1 crates/aura-simulator/src/testing/mod.rs:99:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/testing/mod.rs:98:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/testing/mod.rs:97:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/testing/mod.rs:158:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/testing/mod.rs:100:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/testing/functional_runner.rs:157:39: warning: manual implementation of `.is_multiple_of()`: help: replace with: `self.world.current_tick.is_multiple_of(checkpoint_interval)`
      1 crates/aura-simulator/src/state/snapshot.rs:45:5: warning: missing documentation for an associated type
      1 crates/aura-simulator/src/state/mod.rs:69:5: warning: missing documentation for an associated type
      1 crates/aura-simulator/src/state/mod.rs:29:5: warning: missing documentation for an associated type
      1 crates/aura-simulator/src/state/mod.rs:28:5: warning: missing documentation for an associated type
      1 crates/aura-simulator/src/state/mod.rs:238:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:238:24: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/mod.rs:235:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:235:15: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/mod.rs:232:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:229:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:229:20: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/mod.rs:226:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:226:26: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/mod.rs:223:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/state/mod.rs:223:24: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/mod.rs:174:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/state/mod.rs:114:5: warning: missing documentation for an associated type
      1 crates/aura-simulator/src/state/manager.rs:186:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/state/diff.rs:70:18: warning: redundant closure: help: replace the closure with the function itself: `DiffOperation::from_diff_entry`
      1 crates/aura-simulator/src/state/diff.rs:405:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/diff.rs:404:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/diff.rs:401:14: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/diff.rs:398:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/diff.rs:397:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/diff.rs:278:38: warning: length comparison to zero: help: using `!is_empty` is clearer and more explicit: `!changes.is_empty()`
      1 crates/aura-simulator/src/state/checkpoint.rs:63:17: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:61:28: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:59:18: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:57:19: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:55:17: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:53:14: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/state/checkpoint.rs:344:28: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/state/checkpoint.rs:332:28: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/state/checkpoint.rs:309:9: warning: match expression looks like `matches!` macro: help: try: `matches!((reason, reason_type), (CheckpointCreationReason::Manual { .. }, "manual") | (CheckpointCreationReason::Automatic { .. }, "automatic") | (CheckpointCreationReason::BeforeEvent { .. }, "before_event") | (CheckpointCreationReason::AfterEvent { .. }, "after_event") | (CheckpointCreationReason::BeforeRiskyOperation { .. }, "before_risky") | (CheckpointCreationReason::Emergency { .. }, "emergency"))`
      1 crates/aura-simulator/src/state/checkpoint.rs:284:28: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/simulation_engine.rs:93:27: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/simulation_engine.rs:243:34: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/scenario/types.rs:9:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:97:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:96:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:94:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:90:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:8:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:88:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:86:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:84:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:82:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:80:1: warning: missing documentation for an enum
      1 crates/aura-simulator/src/scenario/types.rs:76:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:75:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:74:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:73:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:72:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:71:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:69:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:65:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:64:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:63:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:62:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:61:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:60:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:59:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:55:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:54:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:53:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:51:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:47:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:46:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:45:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:44:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:40:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:39:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:38:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:37:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:33:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:32:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:31:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:30:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:29:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:28:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:24:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:23:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:22:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:21:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:20:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:19:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:18:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:17:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:16:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:15:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:14:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:142:40: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/scenario/types.rs:13:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:132:5: warning: missing documentation for a method
      1 crates/aura-simulator/src/scenario/types.rs:128:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:127:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:126:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:125:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/types.rs:121:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:121:25: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:119:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:117:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:115:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:113:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/types.rs:111:1: warning: missing documentation for an enum
      1 crates/aura-simulator/src/scenario/types.rs:107:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:106:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:105:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:104:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:103:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/types.rs:101:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/scenario/mod.rs:12:1: warning: missing documentation for a module
      1 crates/aura-simulator/src/scenario/loader.rs:493:32: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/scenario/loader.rs:243:13: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/scenario/loader.rs:180:40: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:180:22: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:176:24: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:172:17: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:167:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:165:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:164:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:158:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:156:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:155:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:154:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:148:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:146:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:145:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:144:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:138:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:136:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:135:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:134:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:133:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/loader.rs:132:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:52:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:224:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/engine.rs:222:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/engine.rs:220:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/engine.rs:220:25: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:218:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/engine.rs:216:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/scenario/engine.rs:173:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:172:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:167:40: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:167:22: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:163:24: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:159:17: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:154:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:153:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:152:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:146:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:145:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:144:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:143:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:137:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:136:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:135:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:134:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:128:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:127:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/engine.rs:126:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:91:14: warning: use of `or_insert_with` to construct default value: help: try: `or_default()`
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:72:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:61:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:60:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:59:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/scenario/choreography_actions.rs:123:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/results/mod.rs:327:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/results/error.rs:345:70: warning: the `Err`-variant returned from this function is very large: the `Err`-variant is at least 160 bytes
      1 crates/aura-simulator/src/quint/types.rs:27:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/types.rs:24:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/types.rs:21:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/types.rs:18:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/types.rs:15:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/trace_converter.rs:88:5: warning: you should consider adding a `Default` implementation for `ItfTraceConverter`
      1 crates/aura-simulator/src/quint/trace_converter.rs:408:34: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/quint/trace_converter.rs:37:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/trace_converter.rs:34:13: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/trace_converter.rs:32:11: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/trace_converter.rs:26:14: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/properties.rs:31:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/properties.rs:28:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/properties.rs:25:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/properties.rs:22:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/properties.rs:19:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:866:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/quint/evaluator.rs:823:16: warning: length comparison to zero: help: using `!is_empty` is clearer and more explicit: `!honest_participants.is_empty()`
      1 crates/aura-simulator/src/quint/evaluator.rs:473:9: warning: this `if` statement can be collapsed
      1 crates/aura-simulator/src/quint/evaluator.rs:37:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:34:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:31:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:28:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:28:43: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/evaluator.rs:28:25: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/evaluator.rs:25:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:22:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/evaluator.rs:19:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:96:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:95:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:94:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:88:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:87:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:86:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:80:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:79:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:78:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:72:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:72:28: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:72:14: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:70:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:70:40: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:70:18: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:67:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:66:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:64:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:63:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:60:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:59:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:57:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:56:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:48:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:47:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:41:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:40:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:39:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/cli_runner.rs:30:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:27:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:24:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:21:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/cli_runner.rs:18:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/chaos_generator.rs:383:35: warning: `to_string` applied to a type that implements `Display` in `format!` args: help: remove this
      1 crates/aura-simulator/src/quint/chaos_generator.rs:367:23: warning: `to_string` applied to a type that implements `Display` in `format!` args: help: remove this
      1 crates/aura-simulator/src/quint/chaos_generator.rs:356:23: warning: `to_string` applied to a type that implements `Display` in `format!` args: help: remove this
      1 crates/aura-simulator/src/quint/chaos_generator.rs:32:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/chaos_generator.rs:29:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/chaos_generator.rs:26:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/chaos_generator.rs:23:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/chaos_generator.rs:20:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:613:9: warning: calls to `push` immediately after creation: help: consider using the `vec![]` macro: `let scenarios = vec![..];`
      1 crates/aura-simulator/src/quint/bridge.rs:57:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:54:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:54:39: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/bridge.rs:54:25: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/bridge.rs:51:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:509:56: warning: this `if` has identical blocks
      1 crates/aura-simulator/src/quint/bridge.rs:48:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:45:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:42:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/bridge.rs:39:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:49:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:48:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:47:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:46:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:45:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:44:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:38:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:37:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:372:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:371:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:36:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:35:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:34:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:33:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:32:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:31:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/quint/ast_parser.rs:22:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:19:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/quint/ast_parser.rs:16:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:90:1: warning: large size difference between variants: the entire enum is at least 232 bytes
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:508:27: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:355:25: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:338:37: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:329:41: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:309:49: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:261:29: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:237:29: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/time_travel_debugger.rs:174:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/observability/passive_trace_recorder.rs:434:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/observability/observability_engine.rs:629:27: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/observability/observability_engine.rs:609:12: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/observability/observability_engine.rs:489:72: warning: this expression creates a reference which is immediately dereferenced by the compiler: help: change this to: `events`
      1 crates/aura-simulator/src/observability/observability_engine.rs:413:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/observability/observability_engine.rs:217:23: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:215:16: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:212:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:211:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:208:25: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:190:21: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:187:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:186:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:182:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:181:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:177:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:176:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:126:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:125:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:124:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:120:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:119:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:118:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:117:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:113:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:112:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/observability_engine.rs:111:9: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/observability/checkpoint_manager.rs:131:25: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/registry.rs:216:1: warning: missing documentation for a macro
      1 crates/aura-simulator/src/metrics/registry.rs:209:1: warning: missing documentation for a macro
      1 crates/aura-simulator/src/metrics/registry.rs:202:1: warning: missing documentation for a macro
      1 crates/aura-simulator/src/metrics/registry.rs:195:1: warning: missing documentation for a macro
      1 crates/aura-simulator/src/metrics/registry.rs:175:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/metrics/registry.rs:171:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/metrics/registry.rs:167:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/metrics/registry.rs:163:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/metrics/registry.rs:154:29: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/registry.rs:147:29: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/registry.rs:140:29: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/registry.rs:133:33: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/registry.rs:126:29: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/mod.rs:46:5: warning: missing documentation for an associated function
      1 crates/aura-simulator/src/metrics/mod.rs:406:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:405:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:404:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:403:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:402:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:401:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:400:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:399:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:398:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:397:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/metrics/mod.rs:336:14: warning: use of `or_insert_with` to construct default value: help: try: `or_default()`
      1 crates/aura-simulator/src/metrics/mod.rs:246:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/metrics/mod.rs:189:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/metrics/mod.rs:176:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/metrics/collector.rs:93:23: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/collector.rs:169:17: warning: this `if let` can be collapsed into the outer `if let`
      1 crates/aura-simulator/src/metrics/collector.rs:128:20: warning: deref which would be done by auto-deref: help: try: `&mut metrics`
      1 crates/aura-simulator/src/metrics/collector.rs:127:31: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/collector.rs:118:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/metrics/collector.rs:111:31: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:53:5: warning: missing documentation for a method
      1 crates/aura-simulator/src/logging.rs:52:5: warning: missing documentation for a method
      1 crates/aura-simulator/src/logging.rs:51:5: warning: missing documentation for a method
      1 crates/aura-simulator/src/logging.rs:50:5: warning: missing documentation for a method
      1 crates/aura-simulator/src/logging.rs:49:1: warning: missing documentation for a trait
      1 crates/aura-simulator/src/logging.rs:46:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:45:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:458:13: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:451:29: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:44:1: warning: missing documentation for an enum
      1 crates/aura-simulator/src/logging.rs:447:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:446:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:436:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:40:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:405:21: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:404:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:39:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:38:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:388:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:37:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:36:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:35:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:34:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:33:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:32:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/logging.rs:31:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/logging.rs:293:10: warning: parameter is only used in recursion
      1 crates/aura-simulator/src/logging.rs:287:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:286:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:285:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:27:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:26:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:25:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:24:1: warning: missing documentation for an enum
      1 crates/aura-simulator/src/logging.rs:246:21: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:245:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:213:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:20:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:19:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:197:21: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:18:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:187:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:17:5: warning: missing documentation for a variant
      1 crates/aura-simulator/src/logging.rs:177:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:16:1: warning: missing documentation for an enum
      1 crates/aura-simulator/src/logging.rs:167:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/logging.rs:157:22: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/config/traits.rs:339:23: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/config/mod.rs:191:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/config/mod.rs:127:1: warning: this `impl` can be derived
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:831:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:496:30: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:484:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:424:24: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:409:60: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:385:13: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:367:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/minimal_reproduction.rs:341:9: warning: calls to `push` immediately after creation: help: consider using the `vec![]` macro: `let strategies: Vec<Box<dyn ParameterVariationStrategy>> = vec![..];`
      1 crates/aura-simulator/src/analysis/focused_tester.rs:908:25: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:890:25: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:853:24: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:838:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:535:24: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:506:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/focused_tester.rs:28:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:27:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:26:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:25:1: warning: missing documentation for a struct
      1 crates/aura-simulator/src/analysis/focused_tester.rs:21:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:20:5: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:133:38: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:133:28: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:133:18: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:131:40: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:131:30: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/focused_tester.rs:131:20: warning: missing documentation for a struct field
      1 crates/aura-simulator/src/analysis/failure_analyzer.rs:567:35: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/failure_analyzer.rs:1185:36: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/failure_analyzer.rs:1129:38: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1903:9: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1889:13: warning: used `unwrap()` on an `Option` value
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1885:39: warning: writing `&mut Vec` instead of `&mut [_]` involves a new object where a slice will do: help: change this to: `&mut [DebuggingInsight]`
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1470:9: warning: calls to `push` immediately after creation: help: consider using the `vec![]` macro: `let resources = vec![..];`
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1430:9: warning: calls to `push` immediately after creation: help: consider using the `vec![]` macro: `let visualizations = vec![..];`
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1133:24: warning: used `unwrap()` on a `Result` value
      1 crates/aura-simulator/src/analysis/debug_reporter.rs:1092:26: warning: used `unwrap()` on a `Result` value
      1 crates/aura-protocol/src/protocols/choreographic/trace.rs:335:5: warning: you should consider adding a `Default` implementation for `ChoreoTraceRecorder`
      1 crates/aura-protocol/src/protocols/choreographic/timeout_management.rs:98:5: warning: you should consider adding a `Default` implementation for `TimeoutManager`
      1 crates/aura-protocol/src/protocols/choreographic/production_example.rs:56:5: warning: enclosing `Ok` and `?` operator are unneeded
      1 crates/aura-protocol/src/protocols/choreographic/middleware_integration.rs:163:10: warning: very complex type used. Consider factoring parts into `type` definitions
      1 crates/aura-protocol/src/protocols/choreographic/error_handling.rs:134:5: warning: you should consider adding a `Default` implementation for `ByzantineDetector`
      1 crates/aura-protocol/src/middleware/event_watcher.rs:365:9: warning: this `map_or` can be simplified
      1 crates/aura-protocol/src/lib.rs:211:1: warning: item has both inner and outer attributes
      1 crates/aura-protocol/src/handlers.rs:36:5: warning: you should consider adding a `Default` implementation for `HandlerBuilder<H>`
      1 crates/aura-protocol/src/effects/console.rs:118:5: warning: you should consider adding a `Default` implementation for `RecordingConsoleEffects`
      1 crates/aura-journal/src/capability/group_capabilities.rs:836:49: warning: used `expect()` on a `Result` value
      1 crates/aura-agent/src/utils/validation.rs:92:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:78:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:67:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:52:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:41:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:35:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/validation.rs:23:55: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/utils/time.rs:7:5: warning: use of a disallowed method `std::time::SystemTime::now`
      1 crates/aura-agent/src/utils/time.rs:15:5: warning: use of a disallowed method `std::time::SystemTime::now`
      1 crates/aura-agent/src/utils/id_gen.rs:7:24: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/utils/id_gen.rs:17:23: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/utils/id_gen.rs:12:29: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/transport_adapter.rs:93:25: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/transport_adapter.rs:135:33: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/traits.rs:265:9: warning: this loop could be written as a `while let` loop: help: try: `while let Ok(Some(envelope)) = self.inner.receive(self.receive_timeout).await { .. }`
      1 crates/aura-agent/src/traits.rs:247:25: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/storage_adapter.rs:281:13: warning: this loop could be written as a `for` loop: help: try: `for result in iter`
      1 crates/aura-agent/src/storage_adapter.rs:243:13: warning: this loop could be written as a `for` loop: help: try: `for result in iter`
      1 crates/aura-agent/src/device_secure_store/store_interface.rs:88:51: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/device_secure_store/macos.rs:174:23: warning: the borrowed expression implements the required traits: help: change this to: `["SPHardwareDataType", "-xml"]`
      1 crates/aura-agent/src/agent/session/trait_impls.rs:455:12: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/agent/session/trait_impls.rs:247:23: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/agent/session/storage_ops.rs:280:31: warning: useless use of `format!`: help: consider using `.to_string()`: `"replica:".to_string()`
      1 crates/aura-agent/src/agent/session/storage_ops.rs:119:62: warning: this expression creates a reference which is immediately dereferenced by the compiler: help: change this to: `data_id`
      1 crates/aura-agent/src/agent/session/state_impls.rs:262:27: warning: use of a disallowed method `std::time::SystemTime::now`
      1 crates/aura-agent/src/agent/session/state_impls.rs:160:38: warning: use of a disallowed method `uuid::Uuid::new_v4`
      1 crates/aura-agent/src/agent/session/state_impls.rs:145:26: warning: use of a disallowed method `std::time::Instant::now`
      1 crates/aura-agent/src/agent/session/identity.rs:242:39: warning: the borrowed expression implements the required traits: help: change this to: `commitment`
      1 crates/aura-agent/src/agent/session/identity.rs:204:39: warning: the borrowed expression implements the required traits: help: change this to: `commitment`
      1 crates/aura-agent/src/agent/core.rs:34:27: warning: very complex type used. Consider factoring parts into `type` definitions
      1 crates/aura-agent/src/agent/core.rs:166:56: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/agent/core.rs:144:56: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/agent/core.rs:130:60: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/agent/core.rs:114:56: warning: the borrowed expression implements the required traits: help: change this to: `format!("Failed to list sessions: {}", e)`
      1 crates/aura-agent/src/agent/core.rs:102:60: warning: the borrowed expression implements the required traits
      1 crates/aura-agent/src/agent/capabilities.rs:84:9: warning: use of a disallowed method `uuid::Uuid::new_v4`
