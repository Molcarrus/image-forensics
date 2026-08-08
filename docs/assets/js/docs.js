/* Progressive enhancement for the docs site. Everything here is optional:
   the pages are fully readable and navigable without it. */

(() => {
    "use strict";

    /* --- heading anchors -------------------------------------------------
       The markdown renderer emits no `id` attributes, so in-page fragment
       links (`/docs/configuration/#minimum-image-sizes`) had nothing to
       target. Derive stable slugs from the heading text. */

    const slugify = (text) =>
        text
            .toLowerCase()
            .replace(/[`'"’]/g, "")
            .replace(/[^a-z0-9]+/g, "-")
            .replace(/^-+|-+$/g, "");

    const addHeadingAnchors = (root) => {
        const used = new Set();
        const headings = root.querySelectorAll("h2, h3");

        headings.forEach((heading) => {
            if (!heading.id) {
                let slug = slugify(heading.textContent || "");
                if (!slug) return;

                let unique = slug;
                let n = 2;
                while (used.has(unique)) unique = `${slug}-${n++}`;
                heading.id = unique;
            }
            used.add(heading.id);

            const anchor = document.createElement("a");
            anchor.className = "heading-anchor";
            anchor.href = `#${heading.id}`;
            anchor.textContent = "#";
            anchor.setAttribute("aria-label", `Link to ${heading.textContent.trim()}`);
            heading.appendChild(anchor);
        });

        return headings;
    };

    /* --- on this page ---------------------------------------------------- */

    const buildToc = (container, headings) => {
        const targets = [...headings].filter((h) => h.id);
        if (targets.length < 3) return;

        const nav = container.querySelector("nav");
        const list = document.createElement("ul");

        targets.forEach((heading) => {
            const item = document.createElement("li");
            const link = document.createElement("a");

            link.href = `#${heading.id}`;
            // Drop the trailing "#" the anchor added.
            link.textContent = heading.firstChild ? heading.firstChild.textContent.trim() : "";
            if (heading.tagName === "H3") link.className = "toc-h3";

            item.appendChild(link);
            list.appendChild(item);
        });

        nav.appendChild(list);
        container.hidden = false;

        // Highlight whichever heading is currently nearest the top.
        const links = new Map(
            [...list.querySelectorAll("a")].map((a) => [a.getAttribute("href").slice(1), a])
        );

        const observer = new IntersectionObserver(
            (entries) => {
                entries.forEach((entry) => {
                    const link = links.get(entry.target.id);
                    if (!link) return;
                    if (entry.isIntersecting) {
                        links.forEach((l) => l.classList.remove("is-active"));
                        link.classList.add("is-active");
                    }
                });
            },
            { rootMargin: "-80px 0px -70% 0px", threshold: 0 }
        );

        targets.forEach((heading) => observer.observe(heading));
    };

    /* --- copy buttons ----------------------------------------------------- */

    const addCopyButtons = (root) => {
        root.querySelectorAll("pre").forEach((pre) => {
            const wrapper = document.createElement("div");
            wrapper.className = "code-block";
            pre.parentNode.insertBefore(wrapper, pre);
            wrapper.appendChild(pre);

            const button = document.createElement("button");
            button.type = "button";
            button.className = "copy-button";
            button.textContent = "Copy";
            button.setAttribute("aria-label", "Copy code to clipboard");

            button.addEventListener("click", async () => {
                try {
                    await navigator.clipboard.writeText(pre.innerText);
                    button.textContent = "Copied";
                    button.dataset.copied = "true";
                } catch {
                    button.textContent = "Press Ctrl+C";
                }

                setTimeout(() => {
                    button.textContent = "Copy";
                    delete button.dataset.copied;
                }, 1800);
            });

            wrapper.appendChild(button);
        });
    };

    /* --- module card categories ------------------------------------------ */

    const tagModuleCards = () => {
        document.querySelectorAll(".module-card").forEach((card) => {
            const chip = card.querySelector(".cat");
            if (chip) card.dataset.cat = chip.textContent.trim().toLowerCase();
        });
    };

    /* --- nav disclosure --------------------------------------------------- */

    const wireDisclosures = () => {
        const menus = [...document.querySelectorAll(".nav-disclosure")];
        if (!menus.length) return;

        // Close on outside click and on Escape; a bare <details> stays open.
        document.addEventListener("click", (event) => {
            menus.forEach((menu) => {
                if (menu.open && !menu.contains(event.target)) menu.open = false;
            });
        });

        document.addEventListener("keydown", (event) => {
            if (event.key !== "Escape") return;
            menus.forEach((menu) => {
                if (menu.open) {
                    menu.open = false;
                    menu.querySelector("summary")?.focus();
                }
            });
        });
    };

    /* --- init ------------------------------------------------------------- */

    const init = () => {
        const prose = document.querySelector(".prose");

        if (prose) {
            const headings = addHeadingAnchors(prose);

            const toc = document.querySelector("[data-toc]");
            if (toc) buildToc(toc, headings);

            // Anchors are only assigned now, so a hash in the initial URL had
            // nothing to scroll to when the browser first tried.
            if (location.hash) {
                const target = document.getElementById(decodeURIComponent(location.hash.slice(1)));
                if (target) target.scrollIntoView();
            }
        }

        addCopyButtons(document.querySelector(".site-main") || document.body);
        tagModuleCards();
        wireDisclosures();
    };

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
