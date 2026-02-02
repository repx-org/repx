import Layout from "@theme/Layout"

import { Header, Cards, CardProps } from "@site/src/components/Highlights"

const features: CardProps[] = [
	{
		title: "Reproducible Builds",
		description: "Nix-based dependency management ensures every experiment runs in an identical environment, forever.",
	},
	{
		title: "Parameter Sweeps",
		description: "Define parameter grids declaratively. RepX automatically generates and tracks all combinations.",
	},
	{
		title: "Dependency Graphs",
		description: "Build complex pipelines with automatic dependency resolution between stages.",
	},
	{
		title: "HPC Integration",
		description: "Native support for SLURM, SSH remotes, and container isolation with Docker/Podman.",
	},
	{
		title: "Python Analysis",
		description: "Query results with repx-py. Filter jobs by parameters, load outputs directly into pandas.",
	},
	{
		title: "Terminal UI",
		description: "Monitor progress, inspect logs, and explore outputs with the built-in TUI.",
	},
]

export default function Features(): JSX.Element {
	return (
		<Layout title="Features" description="List of RepX features.">
			<main className="margin-vert--lg">
				<Header
					heading="Features"
					description="Reproducible HPC experiments made simple."
					link={{
						emoji: "✨",
						text: "Suggest a feature!",
						to: "https://github.com/repx-org/repx/issues/new",
					}}
				/>
				<Cards from={features} />
			</main>
		</Layout>
	)
}
