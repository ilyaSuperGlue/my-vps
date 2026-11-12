const n_loop = Number(Bun.argv?.[2] ?? 10);
const rateLimiterTest = async (n: number) => {
  const temp = {
    rejected: 0,
    success: 0,
  };
  for (let i = 0; i < n; i++) {
    const res = await fetch("http://127.0.0.1:8080/v1/status");
    if (res.status === 200) {
      temp.success += 1;
    } else {
      temp.rejected += 1;
    }
  }
  console.log(`loop x${n_loop} , result : ${JSON.stringify(temp)}`);
};

await rateLimiterTest(n_loop);

export {};
