# Create separate workspace for fan-curve-app
mkdir -p ~/.cargo/workspaces/fan-curve-app
cd ~/.cargo/workspaces/fan-curve-app

# Clone and build in isolation
git clone https://github.com/tonysystem76/Fan-Curve-App.git .
cargo build --release --locked
sudo cp target/release/fan-curve-app /usr/local/bin/
