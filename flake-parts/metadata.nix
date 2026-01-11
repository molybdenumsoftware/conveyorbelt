{ lib, config, ... }:
{
  options.metadata = {
    title = lib.mkOption {
      type = lib.types.singleLineStr;
      default = "conveyorbelt";
    };
    description = lib.mkOption {
      type = lib.types.singleLineStr;
      default = "CLI for web development that watches source, invokes rebuild, statically serves and triggers page reload";
    };
  };
  config = {
    flake-file.description = config.metadata.description;

    cargoManifest.package = {
      description = config.metadata.description;
      name = config.metadata.title;
    };
  };
}
