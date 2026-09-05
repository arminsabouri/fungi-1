{ ... }:
{
  perSystem =
    {
      pkgs,
      toolchains,
      ...
    }:
    let
      craneLib = toolchains.nightly;
      workspaceSpecs = import ./workspaces.nix { inherit pkgs; };
      repoRoot = ../.;
      src = craneLib.cleanCargoSource repoRoot;

      mkWorkspace =
        name: spec:
        assert spec ? manifestPath;
        assert spec ? packages;
        assert builtins.isAttrs spec.packages;
        let
          manifest = repoRoot + "/${spec.manifestPath}";
          workspaceDir = builtins.dirOf spec.manifestPath;
          cargoManifestPath = builtins.baseNameOf spec.manifestPath;
          lockfile = repoRoot + "/${workspaceDir}/Cargo.lock";
          manifestArgs = "--manifest-path ${pkgs.lib.escapeShellArg cargoManifestPath}";
          commonArgs = {
            inherit src;
            buildInputs = spec.buildInputs or [ ];
            cargoToml = manifest;
            pname = name;
            strictDeps = true;
            version = "0.0.0";
            cargoExtraArgs = manifestArgs;
            cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = lockfile; };
            postUnpack = ''
              cd "$sourceRoot/${workspaceDir}"
              sourceRoot=.
            '';
          };
          cargoArtifactsRelease = craneLib.buildDepsOnly commonArgs;
          cargoArtifactsDev = craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = "dev"; });
          mkPackage =
            outputName: packageSpec:
            let
              cargoPackage = packageSpec.cargoPackage or outputName;
              extraArgs = packageSpec.cargoExtraArgs or "";
            in
            craneLib.buildPackage (
              commonArgs
              // {
                pname = outputName;
                cargoArtifacts = cargoArtifactsRelease;
                cargoExtraArgs = "${manifestArgs} --package ${pkgs.lib.escapeShellArg cargoPackage} ${extraArgs}";
              }
            );
        in
        assert builtins.pathExists manifest;
        assert builtins.pathExists lockfile;
        {
          inherit
            cargoManifestPath
            cargoArtifactsDev
            cargoArtifactsRelease
            commonArgs
            ;
          manifestPath = spec.manifestPath;
          packages = builtins.mapAttrs mkPackage spec.packages;
        };

      cargoWorkspaces = builtins.mapAttrs mkWorkspace workspaceSpecs;
    in
    {
      _module.args = { inherit cargoWorkspaces; };
      packages = pkgs.lib.concatMapAttrs (_: workspace: workspace.packages) cargoWorkspaces;
    };
}
