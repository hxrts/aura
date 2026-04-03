use aura_macros::tell;

tell!(r#"
module parameterized_roles_and_parallel exposing (ParameterizedRolesAndParallel)

protocol ParameterizedRolesAndParallel =
  roles Coordinator, Workers[N], Auditors[*]

  Coordinator { parallel : true } -> Workers[*] : WorkAssigned

  Workers[0..quorum] -> Coordinator : WorkAck

  choice Coordinator at
    | Commit =>
      Coordinator { parallel : true } -> Auditors[*] : CommitNotice
    | Abort =>
      Coordinator -> Auditors[*] : AbortNotice
"#);

fn main() {}
