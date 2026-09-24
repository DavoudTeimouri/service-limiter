# Project Structure

service-limiter/
├── architecture.md
├── README.md
├── USER_GUIDE.md
├── STRUCTURE.md
├── GITHUB_SETUP.md
├── pyproject.toml
└── service_limiter/
    ├── __init__.py
    ├── orchestrator.py
    ├── platform/
    │   ├── __init__.py
    │   └── detector.py
    ├── cli/
    │   ├── __init__.py
    │   └── main.py
    └── models/
        ├── __init__.py
        ├── service_descriptor.py
        ├── resource_profile.py
        └── shared_state.py
