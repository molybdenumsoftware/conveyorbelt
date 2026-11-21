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
        "http"
        "indoc"
        "maud"
        "notify"
        "serde"
        "serde_json"
        "static_init"
        "strip-ansi-escapes"
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
        static-web-server.version = "3.0.0-beta.3";
        hyper = {
          features = [
            "http1"
            "server"
          ];
          version = "*";
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
          "futures"
          "git2"
          "http"
          "hyper"
          "indoc"
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
          "tracing-subscriber"
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
          "strip-ansi-escapes"
        ]
        (_: {
          workspace = true;
        });
  };
}
