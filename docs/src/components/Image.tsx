import React from "react";
import useBaseUrl from "@docusaurus/useBaseUrl";

export default function Image({
  src,
  alt,
  ...props
}: React.ImgHTMLAttributes<HTMLImageElement>) {
  return <img src={useBaseUrl(src)} alt={alt} {...props} />;
}
