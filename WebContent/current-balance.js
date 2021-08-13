
let previousChart = null;

const loadCurrentBalances = () => {
    const fiat = 'USDT';
    let queryStr = '?fiat=' + fiat;
    if (document.getElementById('sim').checked) {
        queryStr += '&sim=1';
    }

    const url = '/api/balance_history' + queryStr;

    fetch(url)
        .then(response => response.json())
        .then(json => renderBalances(json));
};

const renderBalances = (json) => {
    if (json['success'] != true) {
        console.warn("Can't fetch currenct balance");
        return;
    }

    const currentBalances = json['history'][0];

    const stamp = new Date(currentBalances['stamp']);

    const labels = [];
    const totalBalances = [];
    let totalBalanceSum = 0;

    for (key in currentBalances['currencies']) {
        const balance = currentBalances['currencies'][key];
        const rate = balance['fiat']
        const available = balance['available'] * rate;
        const pending = balance['pending'] * rate;
        const totalBalance = available + pending;

        if (totalBalance > 0) {
            labels.push(balance['symbol'] + '(' + balance['name'] + ')');
            totalBalances.push(totalBalance);
            totalBalanceSum += totalBalance;
        }
    }

    // Clear previous chart
    if (previousChart != null) {
        previousChart.destroy();
    }

    const ctx = document.getElementById('balanceChart').getContext('2d');
    previousChart = new Chart(ctx, {
        type: 'doughnut',
        data: {
            labels: labels,
            datasets: [{
                data: totalBalances
            }]
        },
        options: {
            title: {
                display: true,
                text: 'Total balance'
            },
            plugins: {
                colorschemes: {
                    scheme: 'tableau.Classic20'
                }
            },
            elements: {
                center: {
                    text: totalBalanceSum.toFixed(2) + ' USDT'
                }
            }
        }
    });
};

window.addEventListener("load", () => loadCurrentBalances());
