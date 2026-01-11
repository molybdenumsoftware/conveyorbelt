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
      manifest = rootPath + "/Cargo.toml";
      lockFile = rootPath + "/Cargo.lock";

      sourceFiles = lib.fileset.unions [
        (rootPath + "/common.rs")
        (rootPath + "/src")
        (rootPath + "/tests")
      ];
    };
  };
}
