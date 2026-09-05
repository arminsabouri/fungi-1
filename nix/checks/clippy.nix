{
  perSystem =
    {
      cargoWorkspaces,
      pkgs,
      toolchains,
      ...
    }:
    let
      workspace = import ./workspace.nix { inherit cargoWorkspaces pkgs; };
    in
    {
      checks = workspace.mapWorkspaces "clippy" (
        _: cargoWorkspace:
        toolchains.nightly.cargoClippy (
          (workspace.checkArgs cargoWorkspace)
          // {
            cargoArtifacts = cargoWorkspace.cargoArtifactsDev;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          }
        )
      );
    };
}
