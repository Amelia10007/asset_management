
let previousChart = null;

const loadBalanceHistory = () => {
    const fiat = 'USDT';

    let queryStr = '?';
    queryStr += 'fiat=' + fiat;
    queryStr += '&since=' + document.getElementById('since').valueAsDate.toISOString();
    queryStr += '&until=' + document.getElementById('until').valueAsDate.toISOString();
    queryStr += '&step=' + document.getElementById('step').value;
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

    const labels = [];
    const totalBalanceSums = [];

    for (key in json['history']) {
        const h = json['history'][key];
        const timestamp = h['stamp'];
        const balances = h['currencies'];

        let totalBalanceSum = 0;

        for (key2 in balances) {
            const balance = balances[key2];
            const rate = balance['rate'];
            const available = balance['available'] * rate;
            const pending = balance['pending'] * rate;
            const totalBalance = available + pending;

            if (totalBalance > 0) {
                totalBalanceSum += totalBalance;
            }
        }

        labels.push(timestamp);
        totalBalanceSums.push(totalBalanceSum);
    }

    // Clear previous chart
    if (previousChart != null) {
        previousChart.destroy();
    }

    const canvas = document.getElementById('balanceChart');
    const ctx = canvas.getContext('2d');
    previousChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels: labels,
            datasets: [{
                label: 'Total balance (USDT)',
                fill: 'origin',
                data: totalBalanceSums
            }]
        },
        options: {
            title: {
                display: true,
                text: 'Total balance history'
            },
            scales: {
                yAxes: [
                    {
                        scaleLabel: {
                            display: true,
                            labelString: "Values"
                        }
                    }
                ]
            }
        }
    });
};

const resetForm = () => {
    // To show today's history, 'until' should be tomorrow
    const until = new Date();
    until.setDate(until.getDate() + 1);
    // Show history during a month
    const since = new Date();
    since.setMonth(since.getMonth() - 1);

    document.getElementById('since').valueAsDate = since;
    document.getElementById('until').valueAsDate = until;
    document.getElementById('sim').checked = false;
    document.getElementById('step').selectedIndex = 2;
}

window.addEventListener("load", () => {
    resetForm();
    loadBalanceHistory();
});
