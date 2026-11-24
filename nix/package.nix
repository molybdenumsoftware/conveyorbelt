{
  bash,
  clippy,
  dbus,
  gitMinimal,
  lib,
  libgit2,
  nushell,
  pkg-config,
  rustPlatform,
  ungoogled-chromium,
  westonLite,
  writableTmpDirAsHomeHook,
}:
# TODO use buildRustCrate instead
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "conveyorbelt";
  __structuredAttrs = true;
  inherit ((lib.importTOML (finalAttrs.passthru.projectRoot + "/Cargo.toml")).workspace.package) version;
  cargoLock.lockFile = finalAttrs.passthru.projectRoot + "/Cargo.lock";

  src = lib.fileset.toSource {
    root = finalAttrs.passthru.projectRoot;
    fileset = lib.fileset.unions [
      (lib.fileset.fileFilter (file: lib.elem file.name ["Cargo.toml" "Cargo.lock"]) finalAttrs.passthru.projectRoot)
      (lib.fileset.fileFilter (file: file.hasExt "rs") finalAttrs.passthru.projectRoot)
    ];
  };

  nativeBuildInputs = [pkg-config];
  buildInputs = [libgit2];
  buildType = "debug";
  dontStrip = true;
  nativeCheckInputs = [writableTmpDirAsHomeHook clippy];
  env.CARGO_BUILD_WARNINGS = "deny";

  preCheck = ''
    set -o allexport
    ${lib.toShellVars finalAttrs.passthru.check.env}
    set +o allexport
    cargo clippy --all-targets --all-features
  '';

  passthru = {
    inherit (finalAttrs) pname;
    projectRoot = ../.;
    check.env = {
      NU_EXECUTABLE = lib.getExe nushell;
      _BASH_EXECUTABLE = lib.getExe bash;
      WESTON_EXECUTABLE = lib.getExe westonLite;
      DBUS_DAEMON_EXECUTABLE = lib.getExe' dbus "dbus-daemon";
      DBUS_SESSION_CONFIG_FILE = "${dbus}/share/dbus-1/session.conf";
      GIT_BIN_PATH = "${gitMinimal}/bin";
      CHROMIUM_BIN_PATH = "${ungoogled-chromium}/bin";
    };
  };

  meta = {
    description = "Web dev workflow; stack-agnostic, hand-coded 🦀";
    # reuse this fact by extracting `meta` to a file
    license = lib.licenses.mit;
    # TODO declare platforms and derive flake systems from them?
  };
})
