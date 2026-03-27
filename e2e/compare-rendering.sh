#!/bin/bash
# Compare rendering between Xvfb and our X11 server.
# Run inside the sidecar container:
#   docker compose exec sidecar bash /compare-rendering.sh
#
# Captures screenshots from both servers and shows differences.

set -e

OUT=/tmp/compare
mkdir -p $OUT

# Start Xvfb
pkill Xvfb 2>/dev/null || true
sleep 0.5
Xvfb :98 -screen 0 1024x768x24 &>/dev/null &
sleep 2

compare_app() {
    local name="$1"
    local cmd="$2"
    local args="$3"
    local wait="${4:-5}"

    echo "=== Testing: $name ==="

    # Run on Xvfb
    DISPLAY=:98 $cmd $args &
    local pid1=$!
    sleep $wait
    DISPLAY=:98 import -window root "$OUT/xvfb_${name}.png" 2>/dev/null || true
    kill $pid1 2>/dev/null; wait $pid1 2>/dev/null

    # Run on our server
    DISPLAY=:99 $cmd $args &
    local pid2=$!
    sleep $wait
    # Our server renders to the frontend, so capture the root window
    # (which includes auto-mapped content)
    DISPLAY=:99 import -window root "$OUT/ours_${name}.png" 2>/dev/null || true
    kill $pid2 2>/dev/null; wait $pid2 2>/dev/null

    # Compare
    if [ -f "$OUT/xvfb_${name}.png" ] && [ -f "$OUT/ours_${name}.png" ]; then
        # Get file sizes as proxy for content
        local xvfb_size=$(stat -c%s "$OUT/xvfb_${name}.png" 2>/dev/null || echo 0)
        local ours_size=$(stat -c%s "$OUT/ours_${name}.png" 2>/dev/null || echo 0)
        echo "  Xvfb: ${xvfb_size} bytes"
        echo "  Ours: ${ours_size} bytes"

        # Check non-black pixel count
        local xvfb_pixels=$(identify -verbose "$OUT/xvfb_${name}.png" 2>/dev/null | grep -o "Colors: [0-9]*" | head -1 || echo "Colors: ?")
        local ours_pixels=$(identify -verbose "$OUT/ours_${name}.png" 2>/dev/null | grep -o "Colors: [0-9]*" | head -1 || echo "Colors: ?")
        echo "  Xvfb $xvfb_pixels"
        echo "  Ours $ours_pixels"
    else
        echo "  FAILED to capture one or both screenshots"
    fi
    echo ""
}

echo "Comparing rendering: Xvfb (:98) vs our server (:99)"
echo "====================================================="
echo ""

compare_app "xeyes" "xeyes" "-geometry 200x150" 3
compare_app "xclock" "xclock" "" 3
compare_app "xlogo" "xlogo" "-geometry 100x100" 3
compare_app "zenity" "zenity" "--info --text Hello --title Test" 5
compare_app "xterm" "xterm" "-geometry 40x10" 5

echo "Screenshots saved to $OUT/"
ls -la $OUT/*.png 2>/dev/null
echo ""
echo "Done. Copy screenshots out with:"
echo "  docker cp \$(docker compose ps -q sidecar):/tmp/compare/ ./compare-screenshots/"
