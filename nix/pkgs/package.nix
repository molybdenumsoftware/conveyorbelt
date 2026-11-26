{
  conveyorbeltWorkspace,
  writableTmpDirAsHomeHook,
}:
conveyorbeltWorkspace.rootCrate.build.override {
  runTests = true;
  testInputs = [writableTmpDirAsHomeHook];
}
