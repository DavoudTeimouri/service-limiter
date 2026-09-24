"""Service descriptor model."""

class ServiceDescriptor:
    def __init__(self, name: str, display_name: str, status: str, 
                 start_type: str, path: str, account: str):
        self.name = name
        self.display_name = display_name
        self.status = status
        self.start_type = start_type
        self.path = path
        self.account = account
        self.child_processes = []  # List of PIDs

    def to_dict(self):
        return {
            'name': self.name,
            'display_name': self.display_name,
            'status': self.status,
            'start_type': self.start_type,
            'path': self.path,
            'account': self.account,
            'child_processes': self.child_processes
        }
