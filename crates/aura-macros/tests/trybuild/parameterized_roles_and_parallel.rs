use aura_macros::choreography;

choreography!(r#"
module parameterized_roles_and_parallel exposing (ParameterizedRolesAndParallel)

protocol ParameterizedRolesAndParallel =
  roles Coordinator, Workers[N], Auditors[*]

  @parallel
  Coordinator -> Workers[*] : WorkAssigned

  Workers[0..quorum] -> Coordinator : WorkAck

  case choose Coordinator of
    Commit ->
      @parallel
      Coordinator -> Auditors[*] : CommitNotice
    Abort ->
      Coordinator -> Workers[*] : AbortNotice
"#);

fn main() {}
