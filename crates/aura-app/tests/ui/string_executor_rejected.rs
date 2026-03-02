use aura_app::strong_command_executor;

strong_command_executor!(
    fn bad_executor(_app: (), _cmd: String) -> () {}
);

fn main() {
    let _ = bad_executor((), String::new());
}
