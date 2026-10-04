# Host test support

This unpublished package is repository build infrastructure, outside the Aura
runtime workspace layers. It depends only on std, fs2 and thiserror. Core and
macros consume it only in tests; native testkit reexports its descriptor lock.

One persistent file at workspace target/tests/.aura-trybuild.lock coordinates
all harnesses. Acquisition is bounded and original OS errors are retained.
Closing the descriptor or process termination releases ownership; callers must
never unlink the lock file. Its monotonic host clock bounds contention only.

The forced-process regression lives in aura-testkit/tests/process_lock.rs and
is mandatory in ownership CI alongside exact execution of every guard harness.
