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
      checks = workspace.mapWorkspaces "build" (
        _: cargoWorkspace:
        toolchains.nightly.buildPackage (
          (workspace.checkArgs cargoWorkspace) // { cargoArtifacts = cargoWorkspace.cargoArtifactsRelease; }
        )
      );
    };
}
