lib: {
  description = "Web dev workflow; stack-agnostic, hand-coded 🦀";
  license = lib.licenses.mit;
  platforms = lib.concatLists [
    lib.platforms.linux
    # lib.platforms.darwin # pull request welcome!
  ];
}
