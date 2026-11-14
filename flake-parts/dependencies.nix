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
        "thiserror"
        "tokio-stream"
        "tracing"
      ] (_: { })
      |> lib.mergeAttrs {
        clap.features = [ "derive" ];
        derive_more.features = [
          "as_ref"
          "debug"
          "deref"
          "deref_mut"
          "display"
          "from"
          "from_str"
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
        tokio.features = [
          "io-util"
          "macros"
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
          "serde"
          "serde_json"
          "static-web-server"
          "tempfile"
          "thiserror"
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
}
