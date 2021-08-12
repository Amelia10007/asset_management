const loadHeaderFooter = () => {

    const headerPos = document.getElementById('header');
    const footerPos = document.getElementById('footer');

    const headerRequest = new XMLHttpRequest();
    headerRequest.open('GET', '/header.html');
    headerRequest.onload = () => headerPos.insertAdjacentHTML('afterbegin', headerRequest.response);

    const footerRequest = new XMLHttpRequest();
    footerRequest.open('GET', '/footer.html');
    footerRequest.onload = () => footerPos.insertAdjacentHTML('afterbegin', footerRequest.response);

    headerRequest.send();
    footerRequest.send();
};

window.addEventListener("load", () => loadHeaderFooter());