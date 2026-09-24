# GitHub Setup

## Contributing
1. Fork the repository.
2. Create a feature branch.
3. Commit your changes.
4. Push to the branch.
5. Open a pull request.

## Development Setup
```bash
# Clone your fork
git clone https://github.com/yourname/service-limiter.git
cd service-limiter
# Create virtual environment
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate
# Install dependencies
pip install -e .[dev]
```

## Running Tests
```bash
pytest
```

## Building Executables
```bash
# Windows
pyinstaller --onefile service_limiter/cli/main.py
# Linux
pyinstaller --onefile service_limiter/cli/main.py
```
