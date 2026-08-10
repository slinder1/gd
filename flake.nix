{
  description = "GitHub stacked-PR builder for those who miss Gerrit";

  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{
      flake-parts,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = inputs.nixpkgs.lib.systems.flakeExposed;
      perSystem =
        { pkgs, lib, ... }:
        let
          toml = (lib.importTOML ./Cargo.toml).package;
        in
        {
          packages = rec {
            praddle = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
              pname = toml.name;
              inherit (toml) version;
              cargoLock = {
                lockFile = ./Cargo.lock;
                allowBuiltinFetchGit = true;
              };
              src = ./.;
              nativeBuildInputs = [
                pkgs.installShellFiles
                pkgs.pkg-config
              ];
              buildInputs = [
                pkgs.openssl
              ];
              postInstall = ''
                installShellCompletion --cmd praddle \
                  --bash gen/praddle.bash \
                  --fish gen/praddle.fish \
                  --zsh gen/_praddle
                installManPage gen/*.1
              '';
            });
            default = praddle;
          };
          formatter = pkgs.nixfmt-tree;
        };
    };
}
