import type { SidebarsConfig } from "@docusaurus/plugin-content-docs"

const sidebars: SidebarsConfig = {
	docsSidebar: [
		{
			type: "category",
			label: "Getting Started",
			items: [
				"getting-started/installation",
				"getting-started/quickstart",
				"getting-started/concepts",
			],
		},
		{
			type: "category",
			label: "User Guide",
			items: [
				"user-guide/defining-experiments",
				"user-guide/stages",
				"user-guide/pipelines",
				"user-guide/parameters",
				"user-guide/dependencies",
				"user-guide/building-labs",
			],
		},
		{
			type: "category",
			label: "Running Experiments",
			items: [
				"running-experiments/local-execution",
				"running-experiments/remote-execution",
				"running-experiments/configuration",
				"running-experiments/containerization",
			"running-experiments/tui",
			"running-experiments/garbage-collection",
		],
		},
		{
			type: "category",
			label: "Analyzing Results",
			items: [
				"analyzing-results/python-analysis",
				"analyzing-results/visualization",
			],
		},
		{
			type: "category",
			label: "Examples",
			items: [
				"examples/simple-pipeline",
				"examples/parameter-sweep",
				"examples/impure-incremental",
				"examples/advanced-patterns",
			],
		},
		{
			type: "category",
			label: "Reference",
			items: [
				"reference/cli-reference",
				"reference/nix-functions",
				"reference/python-api",
			],
		},
		{
			type: "category",
			label: "Contributing",
			items: [
				"contributing/development-setup",
				"contributing/architecture",
				"contributing/testing",
				"contributing/release-process",
			],
		},
	],
}

export default sidebars
