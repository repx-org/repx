import React, { useEffect, useState } from "react";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";

interface VersionInfo {
  version: string;
  url: string;
  latest: boolean;
}

export default function VersionSelector(): JSX.Element {
  const { siteConfig } = useDocusaurusContext();
  const currentVersion =
    (siteConfig.customFields?.docsVersion as string) || "latest";
  const [versions, setVersions] = useState<VersionInfo[]>([]);
  const [isOpen, setIsOpen] = useState(false);

  useEffect(() => {
    fetch("/versions.json")
      .then((res) => res.json())
      .then((data: VersionInfo[]) => setVersions(data))
      .catch(() => setVersions([]));
  }, []);

  if (versions.length === 0) {
    return (
      <span className="navbar__item navbar__link version-label">
        {currentVersion}
      </span>
    );
  }

  return (
    <div
      className={`navbar__item dropdown dropdown--hoverable ${isOpen ? "dropdown--show" : ""}`}
      onMouseEnter={() => setIsOpen(true)}
      onMouseLeave={() => setIsOpen(false)}
    >
      <button
        className="navbar__link version-selector-btn"
        onClick={() => setIsOpen(!isOpen)}
        aria-haspopup="true"
        aria-expanded={isOpen}
      >
        {currentVersion} &#x25BE;
      </button>
      <ul className="dropdown__menu">
        {versions.map((v) => {
          const isCurrent =
            v.version === currentVersion ||
            (currentVersion === "latest" && v.version === "latest");
          return (
            <li key={v.version}>
              <a
                className={`dropdown__link ${isCurrent ? "dropdown__link--active" : ""}`}
                href={v.url}
              >
                {v.version}
                {v.latest && v.version !== "latest" ? " (latest)" : ""}
              </a>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
