"use strict";

const filterInput = document.getElementById("license-filter");
const countNode = document.getElementById("license-count");
const listNode = document.getElementById("license-list");
const statusNode = document.getElementById("catalog-status");

let licenses = [];

function setCatalogStatus(message, tone = "") {
  statusNode.textContent = message;
  statusNode.classList.remove("status-error", "status-success");
  if (tone) {
    statusNode.classList.add(tone);
  }
}

function licenseCountLabel(visible, total) {
  const noun = visible === 1 ? "license" : "licenses";
  if (visible === total) {
    return `${visible} ${noun}`;
  }
  return `${visible} of ${total} ${noun}`;
}

function metadataPair(label, value, code = false) {
  const wrapper = document.createElement("div");
  const term = document.createElement("dt");
  const description = document.createElement("dd");

  term.textContent = label;
  if (code) {
    const codeNode = document.createElement("code");
    codeNode.textContent = value;
    description.append(codeNode);
  } else {
    description.textContent = value;
  }
  wrapper.append(term, description);
  return wrapper;
}

function detailPath(slug) {
  return `/licenses/${encodeURIComponent(slug)}`;
}

function licenseRow(license) {
  const article = document.createElement("article");
  const headingGroup = document.createElement("div");
  const heading = document.createElement("h2");
  const link = document.createElement("a");
  const slug = document.createElement("p");
  const slugLabel = document.createElement("span");
  const slugValue = document.createElement("code");
  const metadata = document.createElement("dl");

  article.className = "license-row";
  headingGroup.className = "license-row-heading";
  heading.className = "license-row-title";
  link.className = "license-link";
  link.href = detailPath(license.slug);
  link.textContent = license.title;
  heading.append(link);

  slug.className = "license-row-slug";
  slugLabel.className = "meta-label";
  slugLabel.textContent = "Slug";
  slugValue.textContent = license.slug;
  slug.append(slugLabel, document.createTextNode(" "), slugValue);
  headingGroup.append(heading, slug);

  metadata.className = "license-row-metadata";
  metadata.append(
    metadataPair("Source file", license.source_filename),
    metadataPair("SHA-256", license.sha256, true),
  );
  article.append(headingGroup, metadata);
  return article;
}

function emptyMessage(message) {
  const paragraph = document.createElement("p");
  paragraph.className = "empty-state";
  paragraph.textContent = message;
  return paragraph;
}

function filterLicenses(query) {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) {
    return licenses;
  }
  return licenses.filter((license) =>
    [license.title, license.slug, license.source_filename].some((value) =>
      value.toLocaleLowerCase().includes(normalized),
    ),
  );
}

function renderLicenses() {
  const visible = filterLicenses(filterInput.value);
  const fragment = document.createDocumentFragment();

  countNode.textContent = licenseCountLabel(visible.length, licenses.length);
  if (visible.length === 0) {
    const message = licenses.length === 0
      ? "No licenses have been published yet."
      : "No published licenses match this filter.";
    fragment.append(emptyMessage(message));
  } else {
    visible.forEach((license) => fragment.append(licenseRow(license)));
  }
  listNode.replaceChildren(fragment);
}

function validLicenseSummary(value) {
  return value !== null
    && typeof value === "object"
    && Number.isSafeInteger(value.id)
    && typeof value.slug === "string"
    && typeof value.title === "string"
    && typeof value.source_filename === "string"
    && typeof value.sha256 === "string"
    && Number.isSafeInteger(value.uploaded_at_ms);
}

function errorNotice() {
  const notice = document.createElement("div");
  const message = document.createElement("p");
  const retry = document.createElement("button");

  notice.className = "notice notice-error";
  message.textContent = "The license archive is unavailable right now.";
  retry.className = "button button-secondary";
  retry.type = "button";
  retry.textContent = "Try again";
  retry.addEventListener("click", () => {
    void loadCatalog();
  });
  notice.append(message, retry);
  return notice;
}

async function loadCatalog() {
  filterInput.disabled = true;
  listNode.setAttribute("aria-busy", "true");
  listNode.replaceChildren(emptyMessage("Loading published licenses…"));
  countNode.textContent = "0 licenses";
  setCatalogStatus("Loading the license register…");

  try {
    const response = await fetch("/api/licenses", {
      headers: { Accept: "application/json" },
      credentials: "same-origin",
    });
    if (!response.ok) {
      throw new Error("catalog request failed");
    }
    const payload = await response.json();
    if (!payload || !Array.isArray(payload.licenses) || !payload.licenses.every(validLicenseSummary)) {
      throw new Error("catalog response was invalid");
    }

    licenses = payload.licenses;
    filterInput.disabled = false;
    renderLicenses();
    setCatalogStatus(
      licenses.length === 0 ? "The register is currently empty." : "The license register is ready.",
      "status-success",
    );
  } catch (_error) {
    licenses = [];
    countNode.textContent = "Unavailable";
    listNode.replaceChildren(errorNotice());
    setCatalogStatus("The license register could not be loaded. Try again.", "status-error");
  } finally {
    listNode.setAttribute("aria-busy", "false");
  }
}

filterInput.addEventListener("input", renderLicenses);
void loadCatalog();
