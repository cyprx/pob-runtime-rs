  {                                                                                                                
    description = "pob-runtime-rs — Native Linux runtime for Path of Building";
                                                                                                                   
    inputs = {    
      nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
      rust-overlay = {
        url = "github:oxalica/rust-overlay";
        inputs.nixpkgs.follows = "nixpkgs";
      };
      flake-utils.url = "github:numtide/flake-utils";
    };

    outputs = { self, nixpkgs, rust-overlay, flake-utils }:
      flake-utils.lib.eachDefaultSystem (system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs { inherit system overlays; };

          # Pinned stable Rust with useful components
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          };

          # Libraries needed at *runtime* (not just build time) by wgpu/winit.
          # On NixOS, LD_LIBRARY_PATH must be set or the dynamic linker won't
          # find Vulkan/GL/X11 — they're not in /usr/lib like on normal distros.
          runtimeLibs = with pkgs; [
            vulkan-loader        # wgpu Vulkan backend
            libGL                # wgpu OpenGL-ES fallback
            libxkbcommon         # winit (required, even on X11)
            wayland              # winit Wayland backend
            xorg.libX11          # winit X11 backend
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
          ];
        in
        {
          devShells.default = pkgs.mkShell {
            buildInputs = with pkgs; [
              # Rust
              rustToolchain

              # Build tools
              pkg-config
              cmake          # some crate build scripts need it
              gnumake

              # Networking (for lcurl / curl crate)
              curl
              openssl
              openssl.dev

              # Wayland stack
              wayland
              wayland-protocols
              libxkbcommon

              # X11 stack
              xorg.libX11
              xorg.libXcursor
              xorg.libXi
              xorg.libXrandr
              xorg.libxcb

              # GPU
              vulkan-loader
              vulkan-headers
              vulkan-validation-layers  # helpful for debugging wgpu
              mesa                      # software/OpenGL fallback

              # Fonts (FreeType for glyphon's font loading)
              freetype
              fontconfig

              # Dev utilities
              git        # for submodule operations
              gdb
              lldb
            ] ++ runtimeLibs;

            # Critical on NixOS: wgpu and winit find GPU/display libs here
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;

            # Tell wgpu which backend to prefer; "auto" also works
            WGPU_BACKEND = "vulkan";

            # Helps wgpu find the Vulkan ICD at runtime
            VK_ICD_FILENAMES = "/nix/store/451cz8g2017h7lqwrcyyjamz8slsv9hc-nvidia-x11-590.48.01-6.18.12/share/vulkan/icd.d/nvidia_icd.x86_64.json:${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json";

            shellHook = ''
              echo "pob-runtime dev shell"
              echo "  Rust : $(rustc --version)"
              echo "  Cargo: $(cargo --version)"
              echo ""
              echo "First time setup:"
              echo "  git submodule update --init --recursive"
              echo ""
              echo "Build:   cargo build"
              echo "Run:     cargo run"
              echo "Test:    cargo test"
            '';
          };
        }
      );
  }
