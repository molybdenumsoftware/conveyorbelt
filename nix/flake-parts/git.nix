{lib, ...}: {
  perSystem = psArgs: {
    options.gitignore = lib.mkOption {
      type = lib.types.listOf lib.types.singleLineStr;
      apply = lib.concat ["result"];
    };
    config = {
      gitignore = ["/target"];
      files.file.".gitignore".text = psArgs.config.gitignore |> lib.naturalSort |> lib.concatLines;
    };
  };
}
