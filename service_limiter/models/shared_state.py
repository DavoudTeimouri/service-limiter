"""Shared state for inter-agent communication."""

class SharedState:
    def __init__(self):
        self.platform_info = {}
        self.services = []
        self.profiles = []
        self.limited_services = []
        self.configs = []

    def update_platform(self, info):
        self.platform_info = info

    def update_services(self, services):
        self.services = services

    def update_profiles(self, profiles):
        self.profiles = profiles

    def update_limited_services(self, services):
        self.limited_services = services

    def update_configs(self, configs):
        self.configs = configs

    def to_dict(self):
        return {
            'platform': self.platform_info,
            'services': [s.to_dict() if hasattr(s, 'to_dict') else s for s in self.services],
            'profiles': [p.to_dict() if hasattr(p, 'to_dict') else p for p in self.profiles],
            'limited_services': [ls.to_dict() if hasattr(ls, 'to_dict') else ls for ls in self.limited_services],
            'configs': self.configs
        }
