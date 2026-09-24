# Service Limiter Architecture

## Overview
Cross-platform application to analyze Windows and Linux services and generate resource limiting configurations.

## Topology
Hierarchical (Orchestrator-Subagent) pattern:
- Orchestrator: Main controller, delegates to subagents, synthesizes results.
- Subagents:
  - ServiceDiscovery: Enumerates services and child processes.
  - ResourceProfiler: Measures current resource usage.
  - PolicyEngine: Evaluates against resource policies.
  - ConfigGenerator: Produces OS-specific configuration files.

## Components
- PlatformDetector: Detects OS and architecture.
- SharedState: Structured state passed between agents.
- CLI: Command-line interface (Windows and Linux).
- TUI: Textual user interface (Linux only) with menu.

## Data Flow
1. PlatformDetector identifies OS/arch.
2. ServiceDiscovery collects service and process tree.
3. ResourceProfiler samples CPU, memory, IO.
4. PolicyEngine applies user-defined limits.
5. ConfigGenerator outputs config (e.g., systemd overrides, Windows Job Objects).
6. Orchestrator manages HITL for high-risk changes.

## Failure Handling
- Circuit breakers on each subagent.
- Fallback chain: primary agent → simplified rule-based → human.
- HITL gate for changes affecting >80% resources or >100 services.
