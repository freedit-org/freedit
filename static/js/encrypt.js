(function () {
  var publicKey = document.getElementById("public-key");
  var textToEncrypt = document.getElementById("text-to-encrypt");
  var button = document.getElementById("button");
  var message = document.getElementById("message");
  var encryptedText = document.getElementById("encrypted-text");
  var result = document.getElementById("result");

  var success = function (data) {
    encryptedText.value = arrayBufferToPem(data, "RSA TEXT");
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

    if (publicKey.value.trim() === "")
      return error("Public key must be specified.");

    var publicKeyArrayBuffer = null;
    try {
      publicKeyArrayBuffer = pemToArrayBuffer(publicKey.value.trim());
    } catch (_) {
      return error("Public key is invalid.");
    }

    if (textToEncrypt.value.trim() === "")
      return error("Text to encrypt must be specified.");

    var data = new TextEncoder().encode(textToEncrypt.value);

    rsaEncrypt(data, publicKeyArrayBuffer).then(success, error);
  };

  button.addEventListener("click", process);
})();
