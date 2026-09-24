"""Text User Interface for Service Limiter (Linux only)."""

import curses
import sys
import traceback
from .orchestrator import Orchestrator

def main(stdscr):
    # Initialize curses
    curses.curs_set(0)  # Hide cursor
    stdscr.nodelay(1)   # Non-blocking input
    stdscr.timeout(100) # Refresh every 100ms

    # Colors
    curses.start_color()
    curses.init_pair(1, curses.COLOR_CYAN, curses.COLOR_BLACK)
    curses.init_pair(2, curses.COLOR_GREEN, curses.COLOR_BLACK)
    curses.init_pair(3, curses.COLOR_YELLOW, curses.COLOR_BLACK)
    curses.init_pair(4, curses.COLOR_RED, curses.COLOR_BLACK)

    orchestrator = Orchestrator()
    analysis_result = None
    selected_idx = 0
    menu_items = ["Analyze Services", "View Service Profiles", "Apply Limits (Placeholder)", "Quit"]
    current_menu = 0
    showing_services = False
    services_list = []

    while True:
        stdscr.clear()
        h, w = stdscr.getmaxyx()

        # Title
        title = "Service Limiter TUI"
        stdscr.addstr(0, (w - len(title)) // 2, title, curses.color_pair(1) | curses.A_BOLD)

        if showing_services:
            # Show services list
            stdscr.addstr(2, 0, "Services (press 'b' to go back):", curses.color_pair(2) | curses.A_BOLD)
            if not services_list:
                stdscr.addstr(4, 0, "No services analyzed yet. Run analysis first.", curses.color_pair(3))
            else:
                for idx, (svc_name, profile) in enumerate(services_list):
                    display_text = f"{svc_name}: CPU={profile.cpu_percent:.1f}% Mem={profile.memory_mb:.1f}MB IO_R={profile.io_read_kbps:.1f}KB/s IO_W={profile.io_write_kbps:.1f}KB/s"
                    if idx == selected_idx:
                        stdscr.addstr(4 + idx, 0, display_text[:w-1], curses.color_pair(4) | curses.A_REVERSE)
                    else:
                        stdscr.addstr(4 + idx, 0, display_text[:w-1], curses.color_pair(2))
        else:
            # Show main menu
            for idx, item in enumerate(menu_items):
                x = w//2 - len(item)//2
                y = h//2 - len(menu_items)//2 + idx
                if idx == current_menu:
                    stdscr.addstr(y, x, item, curses.color_pair(2) | curses.A_REVERSE)
                else:
                    stdscr.addstr(y, x, item, curses.color_pair(2))

        stdscr.refresh()

        # Handle input
        key = stdscr.getch()

        if key == ord('q') or key == ord('Q'):
            break

        if showing_services:
            if key == ord('b') or key == ord('B'):
                showing_services = False
                selected_idx = 0
            elif key == curses.KEY_UP and services_list:
                selected_idx = (selected_idx - 1) % len(services_list)
            elif key == curses.KEY_DOWN and services_list:
                selected_idx = (selected_idx + 1) % len(services_list)
        else:
            if key == curses.KEY_UP:
                current_menu = (current_menu - 1) % len(menu_items)
            elif key == curses.KEY_DOWN:
                current_menu = (current_menu + 1) % len(menu_items)
            elif key in [curses.KEY_ENTER, 10, 13]:  # Enter key
                if current_menu == 0:  # Analyze Services
                    stdscr.clear()
                    stdscr.addstr(0, 0, "Analyzing services...", curses.color_pair(2))
                    stdscr.refresh()
                    try:
                        analysis_result = orchestrator.run_analysis()
                        services_list = []
                        for svc in analysis_result.get('services', []):
                            # We need to get the profile for this service from the profiles list
                            # For simplicity, we'll assume the order matches (which it does in our current implementation)
                            pass
                        # Actually, we have services and profiles in the result. Let's pair them by index.
                        services = analysis_result.get('services', [])
                        profiles = analysis_result.get('profiles', [])
                        for i in range(min(len(services), len(profiles))):
                            svc = services[i]
                            prof = profiles[i]
                            services_list.append((
                                svc.get('name', 'unknown'),
                                type('Profile', (), {
                                    'cpu_percent': prof.get('cpu_percent', 0.0),
                                    'memory_mb': prof.get('memory_mb', 0.0),
                                    'io_read_kbps': prof.get('io_read_kbps', 0.0),
                                    'io_write_kbps': prof.get('io_write_kbps', 0.0)
                                })()
                            ))
                        # If we have no profiles, we can still show services with zero values
                        if not profiles and services:
                            for svc in services:
                                services_list.append((
                                    svc.get('name', 'unknown'),
                                    type('Profile', (), {
                                        'cpu_percent': 0.0,
                                        'memory_mb': 0.0,
                                        'io_read_kbps': 0.0,
                                        'io_write_kbps': 0.0
                                    })()
                                ))
                    except Exception as e:
                        stdscr.addstr(2, 0, f"Error: {e}", curses.color_pair(4))
                    stdscr.addstr(4, 0, "Press any key to continue...", curses.color_pair(3))
                    stdscr.nodelay(0)  # Wait for key press
                    stdscr.getch()
                    stdscr.nodelay(1)
                elif current_menu == 1:  # View Service Profiles
                    if analysis_result is None:
                        stdscr.clear()
                        stdscr.addstr(0, 0, "No analysis data available. Run analysis first.", curses.color_pair(3))
                        stdscr.addstr(2, 0, "Press any key to continue...", curses.color_pair(3))
                        stdscr.nodelay(0)
                        stdscr.getch()
                        stdscr.nodelay(1)
                    else:
                        showing_services = True
                        selected_idx = 0
                elif current_menu == 2:  # Apply Limits (Placeholder)
                    stdscr.clear()
                    stdscr.addstr(0, 0, "Apply Limits feature not yet implemented.", curses.color_pair(3))
                    stdscr.addstr(2, 0, "Press any key to continue...", curses.color_pair(3))
                    stdscr.nodelay(0)
                    stdscr.getch()
                    stdscr.nodelay(1)
                elif current_menu == 3:  # Quit
                    break

if __name__ == '__main__':
    curses.wrapper(main)
