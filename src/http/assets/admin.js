let adminToken = "";
let pendingDeleteSlug = "";
let deleteReturnFocus = null;
let sessionEpoch = 0;
let unlockOperationId = 0;
let uploadOperationId = 0;
let refreshOperationId = 0;
let deleteOperationId = 0;

const tokenGate = requiredElement("token-gate");
const tokenForm = requiredElement("token-form");
const tokenInput = requiredElement("admin-token");
const tokenError = requiredElement("token-error");
const manager = requiredElement("manager");
const lockAdminButton = requiredElement("lock-admin");
const uploadForm = requiredElement("upload-form");
const licenseFile = requiredElement("license-file");
const uploadTitle = requiredElement("upload-title");
const uploadSlug = requiredElement("upload-slug");
const uploadStatus = requiredElement("upload-status");
const adminList = requiredElement("admin-list");
const adminCount = requiredElement("admin-count");
const adminStatus = requiredElement("admin-status");
const deleteDialog = requiredElement("delete-dialog");
const deleteLicenseName = requiredElement("delete-license-name");
const deleteStatus = requiredElement("delete-status");
const confirmDelete = requiredElement("confirm-delete");
const cancelDelete = requiredElement("cancel-delete");

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

tokenForm.addEventListener("submit", unlockManager);
lockAdminButton.addEventListener("click", () => lockManager("Locked."));
uploadForm.addEventListener("submit", uploadLicense);
cancelDelete.addEventListener("click", () => deleteDialog.close("cancel"));
confirmDelete.addEventListener("click", deleteLicense);
deleteDialog.addEventListener("cancel", (event) => {
  if (confirmDelete.disabled) {
    event.preventDefault();
  }
});
deleteDialog.addEventListener("close", finishDeleteDialog);

initializeLockedView();

function requiredElement(id) {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing required administrator element: ${id}`);
  }
  return element;
}

function initializeLockedView() {
  tokenGate.hidden = false;
  manager.hidden = true;
  tokenInput.value = "";
  renderLicenses([]);
  tokenInput.focus();
}

async function unlockManager(event) {
  event.preventDefault();
  const candidate = tokenInput.value.trim();
  if (!candidate) {
    setText(tokenError, "Enter an administrator token.");
    tokenInput.focus();
    return;
  }

  const epoch = ++sessionEpoch;
  const operationId = ++unlockOperationId;
  let focusTokenAfterCleanup = false;
  adminToken = candidate;
  tokenInput.value = "";
  setText(tokenError, "Checking access…");
  setFormBusy(tokenForm, true, "Checking…");

  try {
    const response = await authenticatedRequest("/api/admin/licenses", { method: "GET" });
    if (!isCurrentSessionOperation(epoch, operationId, unlockOperationId)) {
      return;
    }
    if (!response.ok) {
      const message = await apiErrorMessage(response, "Unable to verify this token.");
      if (!isCurrentSessionOperation(epoch, operationId, unlockOperationId)) {
        return;
      }
      clearToken();
      setText(tokenError, response.status === 401 ? "Invalid token." : message);
      focusTokenAfterCleanup = true;
      return;
    }

    const licenses = await licenseListFrom(response);
    if (!isCurrentSessionOperation(epoch, operationId, unlockOperationId)) {
      return;
    }
    renderLicenses(licenses);
    tokenGate.hidden = true;
    manager.hidden = false;
    setText(tokenError, "");
    setText(adminStatus, "");
    lockAdminButton.focus();
  } catch {
    if (isCurrentSessionOperation(epoch, operationId, unlockOperationId)) {
      clearToken();
      setText(tokenError, "The administrator service could not be reached. Try again.");
      focusTokenAfterCleanup = true;
    }
  } finally {
    if (isCurrentOperation(epoch, operationId, unlockOperationId)) {
      setFormBusy(tokenForm, false, "Unlock");
      if (focusTokenAfterCleanup) {
        tokenInput.focus();
      }
    }
  }
}

function lockManager(message) {
  sessionEpoch += 1;
  unlockOperationId += 1;
  uploadOperationId += 1;
  refreshOperationId += 1;
  deleteOperationId += 1;
  clearToken();
  pendingDeleteSlug = "";
  tokenForm.reset();
  uploadForm.reset();
  setFormBusy(tokenForm, false, "Unlock");
  setFormBusy(uploadForm, false, "Upload");
  adminList.setAttribute("aria-busy", "false");
  setDeleteBusy(false);
  if (deleteDialog.open) {
    deleteDialog.close("locked");
  }
  renderLicenses([]);
  setText(uploadStatus, "");
  setText(adminStatus, "");
  manager.hidden = true;
  tokenGate.hidden = false;
  setText(tokenError, message);
  tokenInput.focus();
}

function clearToken() {
  adminToken = "";
  tokenInput.value = "";
}

async function uploadLicense(event) {
  event.preventDefault();
  const file = licenseFile.files && licenseFile.files[0];
  if (!file) {
    setText(uploadStatus, "Choose a license file before publishing.");
    licenseFile.focus();
    return;
  }

  const body = new FormData();
  body.append("file", file, file.name);
  appendOptionalField(body, "title", uploadTitle.value);
  appendOptionalField(body, "slug", uploadSlug.value);

  const epoch = sessionEpoch;
  const operationId = ++uploadOperationId;
  setText(uploadStatus, "Uploading…");
  setFormBusy(uploadForm, true, "Uploading…");

  try {
    const response = await authenticatedRequest("/api/admin/licenses", {
      method: "POST",
      body,
    });
    if (!isCurrentSessionOperation(epoch, operationId, uploadOperationId)) {
      return;
    }
    if (response.status === 401) {
      lockManager("Your token is no longer valid. Enter it again.");
      return;
    }
    if (!response.ok) {
      const message = await apiErrorMessage(response, "The license could not be published.");
      if (!isCurrentSessionOperation(epoch, operationId, uploadOperationId)) {
        return;
      }
      setText(uploadStatus, message);
      return;
    }

    uploadForm.reset();
    setText(uploadStatus, `Uploaded ${file.name}.`);
    await refreshLicenses(epoch, "");
  } catch {
    if (isCurrentSessionOperation(epoch, operationId, uploadOperationId)) {
      setText(uploadStatus, "The upload could not be completed. Check the connection and try again.");
    }
  } finally {
    if (isCurrentSessionOperation(epoch, operationId, uploadOperationId)) {
      setFormBusy(uploadForm, false, "Upload");
    }
  }
}

function appendOptionalField(body, name, value) {
  const trimmed = value.trim();
  if (trimmed) {
    body.append(name, trimmed);
  }
}

async function refreshLicenses(epoch, successMessage) {
  if (!isCurrentSession(epoch)) {
    return false;
  }
  const operationId = ++refreshOperationId;
  adminList.setAttribute("aria-busy", "true");
  setText(adminStatus, "Loading…");

  try {
    const response = await authenticatedRequest("/api/admin/licenses", { method: "GET" });
    if (!isCurrentSessionOperation(epoch, operationId, refreshOperationId)) {
      return false;
    }
    if (response.status === 401) {
      lockManager("Your token is no longer valid. Enter it again.");
      return false;
    }
    if (!response.ok) {
      const message = await apiErrorMessage(response, "The registry could not be refreshed.");
      if (!isCurrentSessionOperation(epoch, operationId, refreshOperationId)) {
        return false;
      }
      setText(adminStatus, message);
      return false;
    }

    const licenses = await licenseListFrom(response);
    if (!isCurrentSessionOperation(epoch, operationId, refreshOperationId)) {
      return false;
    }
    renderLicenses(licenses);
    setText(adminStatus, successMessage);
    return true;
  } catch {
    if (isCurrentSessionOperation(epoch, operationId, refreshOperationId)) {
      setText(adminStatus, "The registry could not be refreshed. Try again.");
    }
    return false;
  } finally {
    if (isCurrentSessionOperation(epoch, operationId, refreshOperationId)) {
      adminList.setAttribute("aria-busy", "false");
    }
  }
}

function renderLicenses(licenses) {
  adminList.replaceChildren();
  adminCount.textContent = `${licenses.length} ${licenses.length === 1 ? "license" : "licenses"}`;

  if (licenses.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty-state";
    empty.textContent = "No licenses.";
    adminList.append(empty);
    return;
  }

  for (const license of licenses) {
    adminList.append(createLicenseRow(license));
  }
}

function createLicenseRow(license) {
  const item = document.createElement("li");
  item.className = "admin-row";
  item.setAttribute("role", "listitem");

  const content = document.createElement("div");
  content.className = "license-row-heading";
  const title = license.title;
  const slug = license.slug;

  const heading = document.createElement("h3");
  heading.className = "license-row-title";
  heading.textContent = title;
  content.append(heading);

  const metadata = document.createElement("dl");
  metadata.className = "license-row-metadata";
  appendMetadata(metadata, "Slug", slug);
  appendMetadata(metadata, "File", license.source_filename);
  appendMetadata(metadata, "Published", formatUploadDate(license.uploaded_at_ms));
  content.append(metadata);
  item.append(content);

  if (slug) {
    const removeButton = document.createElement("button");
    removeButton.type = "button";
    removeButton.className = "button button-danger";
    removeButton.textContent = "Delete";
    removeButton.setAttribute("aria-label", `Delete ${title}`);
    removeButton.addEventListener("click", () => openDeleteDialog(slug, title, removeButton));
    item.append(removeButton);
  }

  return item;
}

function appendMetadata(list, label, value) {
  const group = document.createElement("div");
  const term = document.createElement("dt");
  const description = document.createElement("dd");
  term.textContent = label;
  description.textContent = value;
  group.append(term, description);
  list.append(group);
}

function formatUploadDate(value) {
  const date = new Date(Number(value));
  return dateFormatter.format(date);
}

function openDeleteDialog(slug, title, trigger) {
  pendingDeleteSlug = slug;
  deleteReturnFocus = trigger;
  deleteDialog.returnValue = "";
  deleteLicenseName.textContent = title;
  setText(deleteStatus, "");
  setDeleteBusy(false);
  deleteDialog.showModal();
  cancelDelete.focus();
}

async function deleteLicense() {
  if (!pendingDeleteSlug || !adminToken) {
    setText(deleteStatus, "The deletion request is no longer available. Close and try again.");
    return;
  }

  const slug = pendingDeleteSlug;
  const title = deleteLicenseName.textContent;
  const epoch = sessionEpoch;
  const operationId = ++deleteOperationId;
  setDeleteBusy(true);
  setText(deleteStatus, "Deleting license…");

  try {
    const response = await authenticatedRequest(`/api/admin/licenses/${encodeURIComponent(slug)}`, {
      method: "DELETE",
    });
    if (!isCurrentSessionOperation(epoch, operationId, deleteOperationId)) {
      return;
    }
    if (response.status === 401) {
      deleteDialog.close("locked");
      lockManager("Your token is no longer valid. Enter it again.");
      return;
    }
    if (!response.ok) {
      const message = await apiErrorMessage(response, "The license could not be deleted.");
      if (!isCurrentSessionOperation(epoch, operationId, deleteOperationId)) {
        return;
      }
      setText(deleteStatus, message);
      return;
    }

    deleteDialog.close("deleted");
    setText(adminStatus, `Deleted ${title}.`);
    await refreshLicenses(epoch, "");
  } catch {
    if (isCurrentSessionOperation(epoch, operationId, deleteOperationId)) {
      setText(deleteStatus, "The deletion could not be completed. Check the connection and try again.");
    }
  } finally {
    if (isCurrentSessionOperation(epoch, operationId, deleteOperationId)) {
      setDeleteBusy(false);
    }
  }
}

function setDeleteBusy(busy) {
  confirmDelete.disabled = busy;
  cancelDelete.disabled = busy;
  deleteDialog.setAttribute("aria-busy", String(busy));
}

function finishDeleteDialog() {
  const returnFocus = deleteReturnFocus;
  const deleted = deleteDialog.returnValue === "deleted";
  pendingDeleteSlug = "";
  deleteReturnFocus = null;
  deleteLicenseName.textContent = "";
  setText(deleteStatus, "");
  setDeleteBusy(false);

  if (deleted) {
    adminStatus.focus();
  } else if (returnFocus && returnFocus.isConnected && !manager.hidden) {
    returnFocus.focus();
  }
}

async function authenticatedRequest(path, options) {
  if (!adminToken) {
    throw new Error("Administrator session is locked");
  }
  return fetch(path, {
    ...options,
    cache: "no-store",
    headers: {
      Authorization: `Bearer ${adminToken}`,
    },
  });
}

async function licenseListFrom(response) {
  const payload = await response.json();
  if (!payload || !Array.isArray(payload.licenses) || !payload.licenses.every(validLicenseSummary)) {
    throw new Error("Invalid administrator response");
  }
  return payload.licenses;
}

function validLicenseSummary(value) {
  return value !== null
    && typeof value === "object"
    && Number.isSafeInteger(value.id)
    && typeof value.slug === "string"
    && value.slug.length > 0
    && typeof value.title === "string"
    && value.title.length > 0
    && typeof value.source_filename === "string"
    && value.source_filename.length > 0
    && typeof value.sha256 === "string"
    && Number.isSafeInteger(value.uploaded_at_ms)
    && !Number.isNaN(new Date(value.uploaded_at_ms).getTime());
}

async function apiErrorMessage(response, fallback) {
  try {
    const payload = await response.json();
    const message = payload && payload.error && payload.error.message;
    if (typeof message === "string" && message.length > 0 && message.length <= 240) {
      return message;
    }
  } catch {
    // The fallback is intentionally short and does not expose response internals.
  }
  return fallback;
}

function setFormBusy(form, busy, buttonText) {
  form.setAttribute("aria-busy", String(busy));
  for (const control of form.elements) {
    control.disabled = busy;
  }
  const submit = form.querySelector('button[type="submit"]');
  if (submit) {
    submit.textContent = buttonText;
  }
}

function isCurrentSession(epoch) {
  return Boolean(adminToken) && epoch === sessionEpoch;
}

function isCurrentOperation(epoch, operationId, currentOperationId) {
  return epoch === sessionEpoch && operationId === currentOperationId;
}

function isCurrentSessionOperation(epoch, operationId, currentOperationId) {
  return isCurrentSession(epoch) && operationId === currentOperationId;
}

function setText(element, message) {
  element.textContent = message;
}
