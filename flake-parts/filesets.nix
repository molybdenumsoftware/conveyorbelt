{
  lib,
  rootPath,
  ...
}:
{

  options.filesets = lib.mkOption {
    type = lib.types.lazyAttrsOf lib.types.fileset;
  };

  config = {
    _module.args.toSource =
      fileset:
      lib.fileset.toSource {
        root = rootPath;
        inherit fileset;
      };

    filesets = {
      workspaceManifest = rootPath + "/Cargo.toml";
      binManifest = rootPath + "/crates/bin/Cargo.toml";
      lockFile = rootPath + "/Cargo.lock";

      sourceFiles = lib.fileset.unions [
        (rootPath + "/crates/bin/common.rs")
        (rootPath + "/crates/bin/src")
        (rootPath + "/crates/bin/tests")
      ];
    };
  };
}
