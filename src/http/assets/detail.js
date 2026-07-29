"use strict";

const detailStatus = document.getElementById("detail-status");
const detailContent = document.getElementById("detail-content");
const titleNode = document.getElementById("license-title");
const slugNode = document.getElementById("license-slug");
const sourceNode = document.getElementById("license-source");
const digestNode = document.getElementById("license-digest");
const uploadedNode = document.getElementById("license-uploaded");
const bodyNode = document.getElementById("license-body");
const copyButton = document.getElementById("copy-license");
const copyStatus = document.getElementById("copy-status");

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
    && typeof value.source_filename === "string"
    && typeof value.sha256 === "string"
    && Number.isSafeInteger(value.uploaded_at_ms);
}

function formattedUploadTime(milliseconds) {
  const date = new Date(milliseconds);
  if (Number.isNaN(date.getTime())) {
    return "Unavailable";
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
  link.textContent = "Return to the license archive";
  detailStatus.classList.remove("status-success");
  detailStatus.classList.add("status-error", "notice", "notice-error");
  detailStatus.replaceChildren(text, link);
}

function renderLicense(license) {
  licenseBody = license.body;
  titleNode.textContent = license.title;
  slugNode.textContent = license.slug;
  sourceNode.textContent = license.source_filename;
  digestNode.textContent = license.sha256;
  uploadedNode.textContent = formattedUploadTime(license.uploaded_at_ms);
  bodyNode.textContent = license.body;
  copyButton.disabled = false;
  detailContent.hidden = false;
  document.title = `${license.title} — AperipNomos`;
  setDetailStatus("License record loaded.", "status-success");
}

async function loadLicense() {
  const slug = pathSlug();
  if (!slug) {
    showDetailError("This license record was not found.");
    return;
  }

  setDetailStatus("Loading the license record…");
  try {
    const response = await fetch(`/api/licenses/${encodeURIComponent(slug)}`, {
      headers: { Accept: "application/json" },
      credentials: "same-origin",
    });
    if (response.status === 404) {
      showDetailError("This license record was not found.");
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
    showDetailError("The license record could not be loaded. Try again from the archive.");
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
