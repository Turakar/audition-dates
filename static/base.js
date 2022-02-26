import Cookies from './js.cookie.js';

const languageSelector = document.getElementById("language-selector");
languageSelector.addEventListener("change", (ev) => {
    const value = ev.target.value;
    if(value) {
        Cookies.set("language", value, { expires: 100});
        window.location.reload();
    }
});
