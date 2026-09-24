"""Orchestrator for service limiter application."""

import logging
import platform
from .platform.detector import PlatformDetector
from .models.shared_state import SharedState
from .models.service_descriptor import ServiceDescriptor
from .models.resource_profile import ResourceProfile
from .policy_engine import PolicyEngine
from .config_generator import ConfigGenerator

# Import platform-specific modules conditionally
if platform.system() == 'Windows':
    from .platform import windows as platform_impl
elif platform.system() == 'Linux':
    from .platform import linux as platform_impl
else:
    platform_impl = None

logger = logging.getLogger(__name__)

class Orchestrator:
    def __init__(self, policy=None, output_dir=None):
        self.detector = PlatformDetector()
        self.shared_state = SharedState()
        self.policy_engine = PolicyEngine()
        if policy is None:
            self.policy = self.policy_engine.default_policies
        else:
            self.policy = policy
        self.output_dir = output_dir
        if self.output_dir:
            self.config_generator = ConfigGenerator(self.output_dir)
        else:
            self.config_generator = None

    def run_analysis(self) -> dict:
        """Run full analysis pipeline: discover, profile, apply policies, generate configs."""
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

        # 4. Apply policies
        limited_services = self._apply_policies(profiles)
        self.shared_state.update_limited_services(limited_services)
        logger.info(f"Applied policies to {len(limited_services)} services")

        # 5. Generate configurations
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
        """Apply resource policies to profiles.
        Returns a list of limited service descriptors (or just the profiles that need limiting?).
        We'll return a list of tuples (service_name, desired_limits) for services that exceed policy.
        For services within policy, we don't need to limit them.
        """
        limited = []
        for profile in profiles:
            within_policy, reason = self.policy_engine.evaluate(profile, self.policy)
            if not within_policy:
                # This service exceeds the policy; we want to limit it to the policy values.
                desired_limits = {
                    'cpu_percent': self.policy.get('cpu_percent', float('inf')),
                    'memory_mb': self.policy.get('memory_mb', float('inf')),
                    'io_read_kbps': self.policy.get('io_read_kbps', float('inf')),
                    'io_write_kbps': self.policy.get('io_write_kbps', float('inf')),
                }
                limited.append((profile.service_name, desired_limits))
                logger.info(f"Service {profile.service_name} exceeds policy: {reason}")
            else:
                logger.debug(f"Service {profile.service_name} within policy: {reason}")
        return limited

    def _generate_configs(self, limited_services):
        """Generate configuration files for services that need limiting.
        limited_services is a list of (service_name, desired_limits).
        Returns a list of generated config file paths.
        """
        if not self.config_generator or not self.output_dir:
            logger.warning("No output directory set, skipping config generation")
            return []
        generated = []
        for service_name, desired_limits in limited_services:
            # Create a config dict
            config = {
                'service_name': service_name,
                'cpu_limit_percent': desired_limits['cpu_percent'],
                'memory_limit_mb': desired_limits['memory_mb'],
                'io_read_limit_kbps': desired_limits['io_read_kbps'],
                'io_write_limit_kbps': desired_limits['io_write_kbps'],
            }
            try:
                config_path = self.config_generator.write_config(service_name, config)
                generated.append(config_path)
                logger.info(f"Generated config for {service_name} at {config_path}")
            except Exception as e:
                logger.error(f"Failed to generate config for {service_name}: {e}")
        return generated

