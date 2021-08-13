const loadHeaderFooter = () => {

    const headerPos = document.getElementById('header');
    const footerPos = document.getElementById('footer');

    fetch('header.html')
        .then(response => response.text())
        .then(text => headerPos.insertAdjacentHTML('afterbegin', text));
    fetch('footer.html')
        .then(response => response.text())
        .then(text => footerPos.insertAdjacentHTML('afterbegin', text));
};

window.addEventListener("load", () => loadHeaderFooter());