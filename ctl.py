#!/usr/bin/env python3
"""Quick control client for palace-director daemon."""
import socket
import json
import sys

SOCKET_PATH = "/run/user/1337/palace-director-tealc.sock"

def send_command(cmd_data):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(SOCKET_PATH)
    sock.sendall((json.dumps(cmd_data) + '\n').encode())
    response = sock.recv(4096).decode().strip()
    sock.close()
    return json.loads(response)

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: ctl.py <command> [args...]")
        print("Commands:")
        print("  ping                  - Health check")
        print("  status                - Get status")
        print("  say <msg>             - Send to Zulip")
        print("  goal <desc>           - Set a goal")
        print("  exec <model> <prompt> - Execute a task with tools")
        print("  session <target>      - Start a session")
        print("  sessions              - List active sessions")
        print("  steer <id> <guidance> - Steer a session")
        print("  issue <id>            - Work on an issue")
        sys.exit(1)

    cmd = sys.argv[1]

    if cmd == "ping":
        r = send_command({"cmd": "ping"})
    elif cmd == "status":
        r = send_command({"cmd": "status"})
    elif cmd == "say" and len(sys.argv) > 2:
        r = send_command({"cmd": "say", "message": " ".join(sys.argv[2:])})
    elif cmd == "goal" and len(sys.argv) > 2:
        r = send_command({"cmd": "goal", "description": " ".join(sys.argv[2:])})
    elif cmd == "session" and len(sys.argv) > 2:
        target = sys.argv[2]
        director = sys.argv[3] if len(sys.argv) > 3 else None
        data = {"cmd": "session", "target": target}
        if director:
            data["director"] = director
        r = send_command(data)
    elif cmd == "sessions":
        r = send_command({"cmd": "sessions"})
    elif cmd == "steer" and len(sys.argv) > 3:
        r = send_command({"cmd": "steer", "session_id": sys.argv[2], "guidance": " ".join(sys.argv[3:])})
    elif cmd == "issue" and len(sys.argv) > 2:
        r = send_command({"cmd": "issue", "id": sys.argv[2]})
    elif cmd == "exec" and len(sys.argv) > 3:
        model = sys.argv[2]
        prompt = " ".join(sys.argv[3:])
        r = send_command({"cmd": "exec", "model": model, "prompt": prompt})
    else:
        print(f"Unknown command or missing args: {cmd}")
        sys.exit(1)

    print(json.dumps(r, indent=2))
