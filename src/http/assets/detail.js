"use strict";

const detailStatus = document.getElementById("detail-status");
const detailContent = document.getElementById("detail-content");
const titleNode = document.getElementById("license-title");
const sourceNode = document.getElementById("license-source");
const digestNode = document.getElementById("license-digest");
const uploadedNode = document.getElementById("license-uploaded");
const bodyNode = document.getElementById("license-body");
const copyButton = document.getElementById("copy-license");
const copyStatus = document.getElementById("copy-status");
const markdownTags = new Set([
  "A", "BLOCKQUOTE", "BR", "CODE", "DEL", "EM", "H2", "H3", "H4", "H5", "H6",
  "HR", "LI", "OL", "P", "PRE", "STRONG", "TABLE", "TBODY", "TD", "TH", "THEAD",
  "TR", "UL",
]);

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: "long",
  timeStyle: "short",
});

let licenseBody = "";

function pathSlug() {
  const encoded = window.location.pathname.split("/").filter(Boolean).at(-1);
  if (!encoded) {
    return null;
  }
  try {
    return decodeURIComponent(encoded);
  } catch (_error) {
    return null;
  }
}

function validLicenseDetail(value) {
  return value !== null
    && typeof value === "object"
    && Number.isSafeInteger(value.id)
    && typeof value.slug === "string"
    && typeof value.title === "string"
    && typeof value.body === "string"
    && (value.body_format === undefined
      || value.body_format === "markdown"
      || value.body_format === "plain_text")
    && (value.rendered_html === undefined
      || typeof value.rendered_html === "string"
      || value.rendered_html === null)
    && typeof value.source_filename === "string"
    && typeof value.sha256 === "string"
    && Number.isSafeInteger(value.uploaded_at_ms);
}

function formattedUploadTime(milliseconds) {
  const date = new Date(milliseconds);
  if (Number.isNaN(date.getTime())) {
    throw new Error("invalid publication time");
  }
  return dateFormatter.format(date);
}

function setDetailStatus(message, tone = "") {
  detailStatus.textContent = message;
  detailStatus.classList.remove("status-error", "status-success");
  if (tone) {
    detailStatus.classList.add(tone);
  }
}

function showDetailError(message) {
  const text = document.createElement("p");
  const link = document.createElement("a");

  detailContent.hidden = true;
  text.textContent = message;
  link.className = "back-link";
  link.href = "/";
  link.textContent = "Back to licenses";
  detailStatus.classList.remove("status-success");
  detailStatus.classList.add("status-error", "notice", "notice-error");
  detailStatus.replaceChildren(text, link);
}

function renderLicense(license) {
  licenseBody = license.body;
  titleNode.textContent = license.title;
  sourceNode.textContent = license.source_filename;
  digestNode.textContent = license.sha256;
  uploadedNode.textContent = formattedUploadTime(license.uploaded_at_ms);
  renderLicenseBody(license);
  copyButton.disabled = false;
  detailContent.hidden = false;
  document.title = `${license.title} — AperipNomos`;
  setDetailStatus("");
}

function renderLicenseBody(license) {
  const markdown = license.body_format === "markdown"
    && typeof license.rendered_html === "string";
  bodyNode.classList.toggle("markdown-body", markdown);
  if (!markdown) {
    const pre = document.createElement("pre");
    pre.className = "plain-license-text";
    pre.tabIndex = 0;
    pre.setAttribute("aria-label", "License text");
    pre.textContent = license.body;
    bodyNode.replaceChildren(pre);
    return;
  }
  const parsed = new DOMParser().parseFromString(license.rendered_html, "text/html");
  const fragment = document.createDocumentFragment();
  for (const child of parsed.body.childNodes) {
    const safe = safeMarkdownNode(child);
    if (safe) {
      fragment.append(safe);
    }
  }
  bodyNode.replaceChildren(fragment);
}

function safeMarkdownNode(node) {
  if (node.nodeType === Node.TEXT_NODE) {
    return document.createTextNode(node.nodeValue || "");
  }
  if (node.nodeType !== Node.ELEMENT_NODE || !markdownTags.has(node.tagName)) {
    return safeMarkdownChildren(node);
  }

  const copy = document.createElement(node.tagName.toLowerCase());
  copyMarkdownAttributes(node, copy);
  if (sourceIsScrollable(node)) {
    copy.tabIndex = 0;
    copy.setAttribute("aria-label", node.tagName === "PRE" ? "Code block" : "Table");
  }
  copy.append(safeMarkdownChildren(node));
  return copy;
}

function sourceIsScrollable(node) {
  return node.tagName === "PRE" || node.tagName === "TABLE";
}

function safeMarkdownChildren(node) {
  const fragment = document.createDocumentFragment();
  for (const child of node.childNodes) {
    const safe = safeMarkdownNode(child);
    if (safe) {
      fragment.append(safe);
    }
  }
  return fragment;
}

function copyMarkdownAttributes(source, destination) {
  if (source.tagName === "A") {
    const href = safeLink(source.getAttribute("href"));
    if (href) {
      destination.setAttribute("href", href);
      destination.setAttribute("rel", "noopener noreferrer");
    }
    const title = source.getAttribute("title");
    if (title) {
      destination.setAttribute("title", title);
    }
  } else if (source.tagName === "CODE") {
    const className = source.getAttribute("class") || "";
    if (/^language-[a-z0-9_-]+$/i.test(className)) {
      destination.className = className;
    }
  } else if (source.tagName === "OL") {
    const start = source.getAttribute("start") || "";
    if (/^[0-9]+$/.test(start)) {
      destination.setAttribute("start", start);
    }
  }
}

function safeLink(value) {
  if (!value || !/^(https?:|mailto:)/i.test(value)) {
    return "";
  }
  try {
    const parsed = new URL(value);
    return ["http:", "https:", "mailto:"].includes(parsed.protocol) ? parsed.href : "";
  } catch (_error) {
    return "";
  }
}

async function loadLicense() {
  const slug = pathSlug();
  if (!slug) {
    showDetailError("License not found.");
    return;
  }

  setDetailStatus("Loading…");
  try {
    const response = await fetch(`/api/licenses/${encodeURIComponent(slug)}`, {
      headers: { Accept: "application/json" },
      credentials: "same-origin",
    });
    if (response.status === 404) {
      showDetailError("License not found.");
      return;
    }
    if (!response.ok) {
      throw new Error("license request failed");
    }
    const payload = await response.json();
    if (!payload || !validLicenseDetail(payload.license)) {
      throw new Error("license response was invalid");
    }
    renderLicense(payload.license);
  } catch (_error) {
    showDetailError("Could not load this license.");
  }
}

async function copyLicense() {
  copyButton.disabled = true;
  copyStatus.classList.remove("status-error", "status-success");
  copyStatus.textContent = "Copying…";
  try {
    if (!navigator.clipboard || typeof navigator.clipboard.writeText !== "function") {
      throw new Error("clipboard is unavailable");
    }
    await navigator.clipboard.writeText(licenseBody);
    copyStatus.textContent = "Copied.";
    copyStatus.classList.add("status-success");
  } catch (_error) {
    copyStatus.textContent = "Copy failed. Select the text to copy it manually.";
    copyStatus.classList.add("status-error");
  } finally {
    copyButton.disabled = false;
  }
}

copyButton.addEventListener("click", () => {
  void copyLicense();
});
void loadLicense();
