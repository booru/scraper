{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/nixos-23.11";
  };

  outputs = { self, fenix, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system: 
    let
      toolchain = fenix.packages.${system}.stable.toolchain;
      pkgs = nixpkgs.legacyPackages.${system};
    in
    rec {
      devShells.default = pkgs.mkShell {
        nativeBuildInputs =
            [
              pkgs.cargo-nextest
              fenix.packages.${system}.stable.toolchain
              pkgs.openssl
              pkgs.iconv
              pkgs.pkg-config
            ] ++ (if system == "aarch64-darwin" then [
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ] else []);
      };

      nixosModules = rec {
        scraper-rs = import ./service.nix self;
        default = scraper-rs;
      };
      
      packages.default =

        (pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
          withComponents = with pkgs; [
            nixpkgs.cargo-nextest
          ];
        }).buildRustPackage {
          pname = "scraper";
          version = "0.1.7";

          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          buildInputs = [
            pkgs.openssl pkgs.iconv pkgs.pkg-config 
          ] ++ (if system == "aarch64-darwin" then [
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ] else []);

          # disable networked tests
          checkNoDefaultFeatures = true;
          checkFeatures = [ ];

          useNextest = true;
        };
      checks.default = packages.default;
    });
}
