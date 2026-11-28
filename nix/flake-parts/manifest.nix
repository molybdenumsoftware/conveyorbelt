{lib, ...}: {
  config = {
    perSystem = psArgs @ {pkgs, ...}: {
      options.manifests = {
        workspace = lib.mkOption {type = lib.types.toml;};
        bin = lib.mkOption {type = lib.types.toml;};
      };

      config = {
        files.file = {
          "Cargo.toml".source = pkgs.writers.writeTOML "Cargo.toml" psArgs.config.manifests.workspace;
          "crates/bin/Cargo.toml".source = pkgs.writers.writeTOML "Cargo.toml" psArgs.config.manifests.bin;
        };

        manifests = {
          workspace = {
            workspace = {
              package = {
                edition = "2024";
                version = "0.0.0";
                license = pkgs.conveyorbelt.meta.license.spdxId;
              };
              resolver = "3";
              members = ["crates/*"];

              dependencies =
                lib.genAttrs [
                  "chromiumoxide"
                  "futures"
                  "git2"
                  "http"
                  "hyper-util"
                  "indoc"
                  "maud"
                  "serde"
                  "serde_json"
                  "static_init"
                  "strip-ansi-escapes"
                  "tempfile"
                  "tracing"
                ] (_: {})
                |> lib.mergeAttrs {
                  time.features = ["macros" "formatting"];
                  notify.features = ["tokio" "macos_fsevent"];
                  clap.features = ["derive"];
                  derive_more.features = ["debug" "deref" "deref_mut" "display" "from_str"];
                  static-web-server.version = "3.0.0-beta.4";
                  nix.features = ["signal"];
                  process-wrap.features = ["tokio1"];
                  replace_with.features = ["std"];
                  tokio.features = ["io-util" "macros" "process"];
                  tracing-subscriber.features = ["env-filter" "local-time"];
                  anyhow.features = ["backtrace" "std"];
                }
                |> lib.mapAttrs (name: attrs:
                  {
                    default-features = false;
                    version = "*";
                  }
                  // attrs);
            };
          };

          bin = {
            package =
              lib.genAttrs ["edition" "license" "version"] (_: {workspace = true;})
              // {
                name = pkgs.conveyorbelt.crateName;
                inherit (pkgs.conveyorbelt.meta) description;
              };
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
                "hyper-util"
                "indoc"
                "nix"
                "notify"
                "serde"
                "serde_json"
                "static-web-server"
                "tempfile"
                "time"
                "tokio"
                "tracing"
                "tracing-subscriber"
              ]
              (_: {workspace = true;});

            dev-dependencies =
              lib.genAttrs
              [
                "indoc"
                "maud"
                "static_init"
                "strip-ansi-escapes"
              ]
              (_: {workspace = true;});
          };
        };
      };
    };
  };
}
