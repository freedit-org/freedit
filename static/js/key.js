(function () {
  var publicKeyText = document.getElementById("public-key-text");
  var privateKeyText = document.getElementById("private-key-text");
  var privateKeyDownload = document.getElementById("private-key-download");
  var button = document.getElementById("button");
  var message = document.getElementById("message");
  var result = document.getElementById("result");

  var success = function (keys) {
    publicKeyText.value = arrayBufferToPem(
      keys.publicKeyBuffer,
      "RSA PUBLIC KEY",
    );
    privateKeyText.value = arrayBufferToPem(
      keys.privateKeyBuffer,
      "RSA PRIVATE KEY",
    );
    privateKeyDownload.href = window.URL.createObjectURL(
      new Blob([privateKeyText.value], { type: "application/octet-stream" }),
    );
    result.style.display = "block";
    message.innerText = null;
    button.disabled = false;
  };

  var error = function (error) {
    message.innerText = error;
    button.disabled = false;
  };

  var process = function () {
    message.innerText = "Processing...";
    button.disabled = true;
    generateRsaKeys().then(success, error);
  };

  var warn = function () {
    if (privateKey.value === "") return;
    return "Are you sure? Your keys will be lost unless you've saved them.";
  };

  button.addEventListener("click", process);
  window.onbeforeunload = warn;
})();
