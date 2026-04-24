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
        "chromiumoxide"
        "futures"
        "ignore-files"
        "notify"
        "serde"
        "serde_json"
        "static-web-server"
        "tempfile"
        "tokio-stream"
        "tracing"
        "watchexec"
        "watchexec-events"
        "watchexec-filterer-ignore"
      ] (_: { })
      |> lib.mergeAttrs {
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
        process-wrap = {
          features = [ "tokio1" ];
        };
        replace_with.features = [ "std" ];
        rxrust = {
          features = [ "scheduler" ];
          version = "1.0.0-rc.3";
        };
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
        "indoc"
        "maud"
        "static_init"
      ] (_: { })
      |> lib.mergeAttrs {
        sysinfo.features = [ "system" ];
      }
      |> applyDefaults;
  };

  perSystem.buildEnv.RUSTFLAGS = "--cfg tokio_unstable";
}
