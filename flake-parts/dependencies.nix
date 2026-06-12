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
  workspaceManifest.workspace = {
    members = [ "crates/*" ];

    dependencies =
      lib.genAttrs [
        "chromiumoxide"
        "futures"
        "git2"
        "indoc"
        "maud"
        "notify"
        "serde"
        "serde_json"
        "static-web-server"
        "static_init"
        "tempfile"
        "tokio-stream"
        "tracing"
      ] (_: { })
      |> lib.mergeAttrs {
        clap.features = [ "derive" ];
        derive_more.features = [
          "deref"
          "deref_mut"
          "display"
          "from"
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
  };

  binManifest = {
    dependencies =
      lib.genAttrs
        [
          "anyhow"
          "chromiumoxide"
          "clap"
          "derive_more"
          "tracing-subscriber"
          "futures"
          "git2"
          "hyper"
          "nix"
          "notify"
          "replace_with"
          "rxrust"
          "serde"
          "serde_json"
          "static-web-server"
          "tempfile"
          "tokio"
          "tokio-stream"
          "tracing"
        ]
        (_: {
          workspace = true;
        });

    dev-dependencies =
      lib.genAttrs
        [
          "indoc"
          "maud"
          "static_init"
        ]
        (_: {
          workspace = true;
        });
  };

  perSystem.buildEnv.RUSTFLAGS = "--cfg tokio_unstable";
}
