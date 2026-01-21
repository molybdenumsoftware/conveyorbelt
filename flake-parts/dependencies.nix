{ lib, ... }:
let
  applyDefaults = lib.mapAttrs (
    name: attrs:
    {
      default-features = false;
      version = "*";
    }
    // attrs
  );
in
{
  cargoManifest = {
    dependencies =
      lib.genAttrs [
        "ignore-files"
        "serde"
        "serde_json"
        "static-web-server"
        "tempfile"
        "tracing"
        "watchexec"
        "watchexec-events"
        "watchexec-filterer-ignore"
      ] (_: { })
      |> lib.mergeAttrs {
        chromiumoxide.features = [ "tokio-runtime" ];
        clap.features = [ "derive" ];
        derive_more.features = [
          "deref"
          "deref_mut"
        ];
        hyper = {
          features = [
            "http1"
            "server"
          ];
          version = "0";
        };
        nix.features = [ "signal" ];
        tokio.features = [
          "io-util"
          "process"
        ];
        tracing-subscriber.features = [ "env-filter" ];
        anyhow.features = [
          "backtrace"
          "std"
        ];
      }
      |> applyDefaults;

    dev-dependencies =
      lib.genAttrs [
        "futures"
        "indoc"
        "maud"
        "static_init"
      ] (_: { })
      |> lib.mergeAttrs {
        sysinfo.features = [ "system" ];
      }
      |> applyDefaults;
  };
}
