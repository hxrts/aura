# One pure key definition for a future dependency-only Cargo warm tree.
# All paths are the exact producer store paths. Callers must supply every
# fingerprint input; this function does not inspect ambient process state.
input:
let
  required = [
    "platform" "devShellPath" "toolchainPath" "vendorPath" "lockHash"
    "dummyManifestPath" "dummyConfigPath" "package" "profile" "features"
    "target" "rustflags" "cc" "wrapper" "incremental" "buildEnv"
  ];
  missing = builtins.filter (name: !(builtins.hasAttr name input)) required;
  extra = builtins.filter (name: !(builtins.elem name required)) (builtins.attrNames input);
  textFields = builtins.filter
    (name: !(builtins.elem name [ "features" "incremental" "buildEnv" ])) required;
  badText = builtins.filter (name: !(builtins.isString input.${name})) textFields;
  badFeatures = !(builtins.isList input.features)
    || !(builtins.all builtins.isString input.features);
  badIncremental = !(builtins.isBool input.incremental);
  badBuildEnv = !(builtins.isAttrs input.buildEnv)
    || !(builtins.all builtins.isString (builtins.attrValues input.buildEnv));
  canonical = { schema = "aura-cargo-deps-key-v1"; inherit input; };
in
  if !(builtins.isAttrs input) then throw "build-cache-key: input must be an attribute set"
  else if missing != [] then throw "build-cache-key: missing required inputs"
  else if extra != [] then throw "build-cache-key: unexpected inputs"
  else if badText != [] || badFeatures || badIncremental || badBuildEnv then
    throw "build-cache-key: invalid input types"
  else if builtins.any (name: input.${name} == "")
    [ "platform" "devShellPath" "toolchainPath" "vendorPath" "lockHash"
      "dummyManifestPath" "dummyConfigPath" "package" "profile" "target" ] then
    throw "build-cache-key: required identity is empty"
  else builtins.hashString "sha256" (builtins.toJSON canonical)
