import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">
          A framework for reproducible HPC experiments.
        </p>
        <p style={{maxWidth: '640px', margin: '0 auto 1.5rem', opacity: 0.9}}>
          Define experiments in Nix, execute on any target (local, SSH, SLURM),
          analyze results in Python. Environment reproducibility is enforced at
          build time, not left to the user.
        </p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/getting-started/quickstart">
            Get Started
          </Link>
        </div>
      </div>
    </header>
  );
}

const properties = [
  {
    title: 'Hermetic builds',
    text:
      'Nix resolves and locks every software dependency at build time. ' +
      'The resulting Lab artifact is self-contained and runs identically on any Linux machine.',
  },
  {
    title: 'Static validation',
    text:
      'Stage scripts are analyzed during the build. Missing commands, undeclared dependencies, ' +
      'and shell errors fail the build -- not a running cluster job.',
  },
  {
    title: 'Portable execution',
    text:
      'A built Lab includes executables, container images, and host tools. ' +
      'Run locally, over SSH, or submit to SLURM. No Nix required on the target.',
  },
  {
    title: 'Parameter sweeps',
    text:
      'Declare parameter lists and RepX generates the Cartesian product as a job DAG. ' +
      'Change one parameter and only affected stages rebuild.',
  },
  {
    title: 'Incremental execution',
    text:
      'Completed jobs persist across runs. Re-run after a failure and only pending jobs execute. ' +
      '--continue-on-failure keeps independent jobs running.',
  },
  {
    title: 'Structured analysis',
    text:
      'A Python API provides queryable access to results and metadata. ' +
      'Filter jobs by parameter values, load artifacts into Pandas DataFrames.',
  },
];

export default function Home(): JSX.Element {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={`${siteConfig.title}`}
      description="A framework for reproducible HPC experiments">
      <HomepageHeader />
      <main>
        <section className="margin-vert--xl">
          <div className="container">
            <div className="row">
              {properties.map((p, i) => (
                <div key={i} className={clsx('col col--4 margin-bottom--lg')}>
                  <Heading as="h3" style={{fontSize: '1rem'}}>{p.title}</Heading>
                  <p>{p.text}</p>
                </div>
              ))}
            </div>
          </div>
        </section>

      </main>
    </Layout>
  );
}
