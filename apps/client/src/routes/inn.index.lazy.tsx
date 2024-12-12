import { createLazyFileRoute } from '@tanstack/react-router'
import { useEffect, useState } from "react";

export const Route = createLazyFileRoute('/inn/')({
  component: InnsList,
})

function InnsList() {
  const [htmlContent, setHtmlContent] = useState<string | null>(null);

useEffect(() => {
    // Fetch raw HTML from the API
    fetch("http://localhost:3001/inn/0")
      .then((response) => response.text())
      .then((html) => setHtmlContent(html))
      .catch((error) => console.error("Failed to fetch HTML:", error));
  }, []);

  if (!htmlContent) {
    return <p>Loading...</p>;
  }

  return (
    <div
      dangerouslySetInnerHTML={{ __html: htmlContent }}
    ></div>
  )
}
