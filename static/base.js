import Cookies from './js.cookie.js';

const languageSelector = document.getElementById("language-selector");
languageSelector.addEventListener("change", (ev) => {
    const value = ev.target.value;
    if(value) {
        Cookies.set("language", value, { expires: 100});
        window.location.reload();
    }
});

const lang = document.getElementById("data-lang").innerText;

const date_format = Intl.DateTimeFormat(lang, {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
});
const dates = document.getElementsByClassName("date");
for (var i = 0; i < dates.length; i++) {
    var date = Date.parse(dates[i].innerHTML);
    dates[i].innerHTML = date_format.format(date);
}

const time_format = Intl.DateTimeFormat(lang, {
    hour: "numeric",
    minute: "numeric",
});
const times = document.getElementsByClassName("time");
for (var i = 0; i < times.length; i++) {
    var time = Date.parse(times[i].innerHTML);
    times[i].innerHTML = time_format.format(time);
}
