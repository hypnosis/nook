#!/bin/bash

# Control script for clearbar macOS menu bar utility
# Usage: ./ctl.sh [start|stop|restart]

set -o pipefail

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_PATH="$SCRIPT_DIR/clearbar.app"
PROCESS_PATTERN="clearbar.app/Contents/MacOS/clearbar"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to check if app is running
is_running() {
    pgrep -f "$PROCESS_PATTERN" > /dev/null 2>&1
}

# Function to show usage
show_usage() {
    cat << EOF
Usage: $0 [command]

Commands:
  start       Start clearbar application
  stop        Stop clearbar application
  restart     Restart clearbar application (stop then start)

Example:
  $0 start
  $0 stop
  $0 restart
EOF
}

# Function to start the app
start_app() {
    if is_running; then
        echo -e "${YELLOW}clearbar is already running${NC}"
        return 0
    fi

    if [ ! -d "$APP_PATH" ]; then
        echo -e "${RED}Error: clearbar.app not found at $APP_PATH${NC}"
        return 1
    fi

    echo "Starting clearbar..."
    open "$APP_PATH"

    # Wait a moment for the app to launch
    sleep 1

    if is_running; then
        echo -e "${GREEN}clearbar started successfully${NC}"
        return 0
    else
        echo -e "${RED}Failed to start clearbar${NC}"
        return 1
    fi
}

# Function to stop the app
stop_app() {
    if ! is_running; then
        echo -e "${YELLOW}clearbar is not running${NC}"
        return 0
    fi

    echo "Stopping clearbar..."
    pkill -f "$PROCESS_PATTERN" || true

    # Wait a moment for the process to terminate
    sleep 1

    if is_running; then
        echo -e "${YELLOW}clearbar did not stop cleanly, forcing...${NC}"
        pkill -9 -f "$PROCESS_PATTERN" || true
        sleep 1
    fi

    if ! is_running; then
        echo -e "${GREEN}clearbar stopped successfully${NC}"
        return 0
    else
        echo -e "${RED}Failed to stop clearbar${NC}"
        return 1
    fi
}

# Function to restart the app
restart_app() {
    echo "Restarting clearbar..."
    stop_app
    sleep 1
    start_app
}

# Main logic
case "${1:-}" in
    start)
        start_app
        ;;
    stop)
        stop_app
        ;;
    restart)
        restart_app
        ;;
    *)
        if [ -n "${1:-}" ]; then
            echo -e "${RED}Unknown command: $1${NC}"
            echo ""
        fi
        show_usage
        exit 1
        ;;
esac
