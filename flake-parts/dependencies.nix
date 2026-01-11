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
        "serde"
        "serde_json"
        "static-web-server"
        "tempfile"
        "tracing"
      ] (_: { })
      |> lib.mergeAttrs {
        chromiumoxide.features = [ "tokio-runtime" ];
        clap.features = [ "derive" ];
        hyper = {
          features = [
            "http1"
            "server"
          ];
          version = "0";
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
        "futures"
        "indoc"
        "maud"
        "static_init"
      ] (_: { })
      |> lib.mergeAttrs {
        sysinfo.features = [ "system" ];
        derive_more.features = [
          "deref"
          "deref_mut"
        ];
        nix.features = [ "signal" ];
      }
      |> applyDefaults;
  };
}
