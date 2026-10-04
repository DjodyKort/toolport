"""Runs mcpm's `agents` command group inside the mcpm venv for gen.sh.

MCPM_PATCH selects the minimal fixes needed to observe mcpm's intended output where the
reference itself crashes:
  uninstall  BaseAgentTranspiler.clean takes `managed_agents`, but `agents uninstall` passes
             `managed_skills`, a TypeError on every call.
  audit      `audit_skills` reads `.frontmatter.allowed_tools`, which AgentFrontmatter lacks, an
             AttributeError for every agent.
"""

import os


def patch_uninstall():
    from mcpm.skills.agents import transpiler
    from mcpm.skills.agents.transpilers import roo_code

    def accept(cls):
        original = cls.clean

        def clean(self, project_root, managed_agents=None, managed_skills=None):
            return original(self, project_root, managed_agents=managed_agents or managed_skills)

        cls.clean = clean

    accept(transpiler.BaseAgentTranspiler)
    accept(roo_code.RooCodeAgentTranspiler)


def patch_audit():
    import mcpm.commands.agents.audit as audit

    class Frontmatter:
        allowed_tools = None

        def __init__(self, inner):
            self._inner = inner

        def __getattr__(self, key):
            return getattr(self._inner, key)

    class Agent:
        def __init__(self, inner):
            self._inner = inner
            self.frontmatter = Frontmatter(inner.frontmatter)

        def __getattr__(self, key):
            return getattr(self._inner, key)

    original = audit.discover_agents
    audit.discover_agents = lambda repo: [Agent(a) for a in original(repo)]


def main():
    patch = os.environ.get("MCPM_PATCH", "")
    if patch == "uninstall":
        patch_uninstall()
    elif patch == "audit":
        patch_audit()
    from mcpm.commands.agents import agents

    agents()


main()
