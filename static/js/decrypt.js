(function () {
  var privateKey = document.getElementById("private-key");
  var encryptedText = document.getElementById("encrypted-text");
  var button = document.getElementById("button");
  var message = document.getElementById("message");
  var decryptedText = document.getElementById("decrypted-text");
  var result = document.getElementById("result");

  var success = function (data) {
    decryptedText.value = new TextDecoder().decode(data);
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

    if (privateKey.value.trim() === "")
      return error("Private key must be specified.");

    var privateKeyArrayBuffer = null;
    try {
      privateKeyArrayBuffer = pemToArrayBuffer(privateKey.value.trim());
    } catch (_) {
      return error("Private key is invalid.");
    }

    if (encryptedText.value.trim() === "")
      return error("Text to decrypt must be specified.");

    var data = null;
    try {
      data = pemToArrayBuffer(encryptedText.value.trim());
    } catch (_) {
      return error("Encrypted text is invalid.");
    }

    rsaDecrypt(data, privateKeyArrayBuffer).then(success, error);
  };

  button.addEventListener("click", process);
})();
