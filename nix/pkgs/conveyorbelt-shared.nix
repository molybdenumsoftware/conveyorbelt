{
  lib,
  nushell,
  bash,
  westonLite,
  dbus,
  gitMinimal,
  ungoogled-chromium,
}: {
  check.env = {
    NU_EXECUTABLE = lib.getExe nushell;
    _BASH_EXECUTABLE = lib.getExe bash;
    WESTON_EXECUTABLE = lib.getExe westonLite;
    DBUS_DAEMON_EXECUTABLE = lib.getExe' dbus "dbus-daemon";
    DBUS_SESSION_CONFIG_FILE = "${dbus}/share/dbus-1/session.conf";
    GIT_BIN_PATH = "${gitMinimal}/bin";
    CHROMIUM_BIN_PATH = "${ungoogled-chromium}/bin";
  };
}
