"""Orchestrator for service limiter application."""

import logging
import platform
from .platform.detector import PlatformDetector
from .models.shared_state import SharedState
from .models.service_descriptor import ServiceDescriptor
from .models.resource_profile import ResourceProfile

# Import platform-specific modules conditionally
if platform.system() == 'Windows':
    from .platform import windows as platform_impl
elif platform.system() == 'Linux':
    from .platform import linux as platform_impl
else:
    platform_impl = None

logger = logging.getLogger(__name__)

class Orchestrator:
    def __init__(self):
        self.detector = PlatformDetector()
        self.shared_state = SharedState()
        # Note: policy_engine and config_generator are placeholders for now
        # We'll implement them later if needed.

    def run_analysis(self) -> dict:
        """Run full analysis pipeline."""
        logger.info("Starting service analysis")
        # 1. Detect platform
        platform_info = self.detector.detect()
        self.shared_state.update_platform(platform_info)
        logger.info(f"Platform: {platform_info}")

        # 2. Discover services
        services = self._discover_services()
        self.shared_state.update_services(services)
        logger.info(f"Discovered {len(services)} services")

        # 3. Profile resources
        profiles = self._profile_resources(services)
        self.shared_state.update_profiles(profiles)
        logger.info(f"Profiled {len(profiles)} services")

        # 4. Apply policies (placeholder: for now, just pass through)
        limited_services = self._apply_policies(profiles)
        self.shared_state.update_limited_services(limited_services)
        logger.info(f"Applied policies to {len(limited_services)} services")

        # 5. Generate configurations (placeholder)
        configs = self._generate_configs(limited_services)
        self.shared_state.update_configs(configs)
        logger.info(f"Generated {len(configs)} configurations")

        return self.shared_state.to_dict()

    def _discover_services(self):
        if platform_impl is None:
            logger.warning("Platform not supported for service discovery")
            return []
        try:
            return platform_impl.discover_services()
        except Exception as e:
            logger.error(f"Error in service discovery: {e}")
            return []

    def _profile_resources(self, services):
        if platform_impl is None:
            logger.warning("Platform not supported for resource profiling")
            return []
        profiles = []
        for svc in services:
            try:
                profile = platform_impl.profile_resources(svc)
                profiles.append(profile)
            except Exception as e:
                logger.error(f"Error profiling service {svc.name}: {e}")
                # Optionally, we can still add a zero profile or skip
                profiles.append(ResourceProfile(
                    service_name=svc.name,
                    cpu_percent=0.0,
                    memory_mb=0.0,
                    io_read_kbps=0.0,
                    io_write_kbps=0.0
                ))
        return profiles

    def _apply_policies(self, profiles):
        # Placeholder: for now, we just return the same profiles (no limiting applied)
        # In the future, we would compare each profile against a policy and adjust.
        return profiles

    def _generate_configs(self, limited_services):
        # Placeholder: generate empty configs
        # In the future, we would generate OS-specific config files.
        return [{} for _ in limited_services]
