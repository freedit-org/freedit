// JavaScript is unminified and unchanged, copied from https://github.com/galehouse5/rsa-webcrypto-tool
//
// The Unlicense: <http://unlicense.org>

var pemToBase64String = function (value, label) {
    var lines = value.split("\n");
    var base64String = "";

    for (var i = 0; i < lines.length; i++) {
        if (lines[i].startsWith("-----")) continue;
        base64String += lines[i];
    }

    return base64String;
};

var base64StringToArrayBuffer = function (value) {
    var byteString = atob(value);
    var byteArray = new Uint8Array(byteString.length); 

    for (var i = 0; i < byteString.length; i++) {
        byteArray[i] = byteString.charCodeAt(i);
    }

    return byteArray.buffer;
};

var base64StringToPem = function (value, label) {
    var pem = "-----BEGIN {0}-----\n".replace("{0}", label);

    for (var i = 0; i < value.length; i += 64) {
        pem += value.substr(i, 64) + "\n";
    }

    pem += "-----END {0}-----\n".replace("{0}", label);

    return pem;
};

var arrayBufferToBase64String = function (value) {
    var byteArray = new Uint8Array(value);
    var byteString = "";

    for (var i = 0; i < byteArray.byteLength; i++) {
        byteString += String.fromCharCode(byteArray[i]);
    }

    return btoa(byteString);
};

var pemToArrayBuffer = function (value) {
    return base64StringToArrayBuffer(pemToBase64String(value));
};

var arrayBufferToPem = function (value, label) {
    return base64StringToPem(arrayBufferToBase64String(value), label);
};
